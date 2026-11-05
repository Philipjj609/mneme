//! Recall: compile a context package for a question, with no model needed.
//!
//! 1. **Recognize**: BM25 over each anchor's text (name counts triple).
//! 2. **Activate**: two hops of spreading activation along meaningful links, weighted by
//!    relation and confidence. The strongest path counts fully, further paths with diminishing
//!    weight, so hubs don't outrank exact matches.
//! 3. **Package**: ranked items, each with *why it is here*, what is known slot by slot, and
//!    what is **not** known, within a token budget (or one line per item with `brief`).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use crate::frames::{self, Frames};
use crate::model::{AnchorId, Value};
use crate::state::State;

const STOP: &[&str] = &[
    "a",
    "an",
    "the",
    "of",
    "to",
    "and",
    "or",
    "for",
    "in",
    "on",
    "at",
    "by",
    "with",
    "about",
    "from",
    "how",
    "what",
    "which",
    "who",
    "why",
    "when",
    "where",
    "is",
    "are",
    "be",
    "do",
    "does",
    "can",
    "could",
    "should",
    "would",
    "will",
    "i",
    "me",
    "my",
    "we",
    "our",
    "you",
    "your",
    "it",
    "this",
    "that",
    "these",
    "those",
    "based",
    "explain",
    "tell",
    "use",
    "using",
    "best",
    "way",
    "ways",
    "make",
    "give",
    "show",
    "know",
    "want",
    "need",
    "some",
    "any",
    "all",
    "get",
    "good",
    "into",
    "resource",
    "resources",
    "tool",
    "tools",
    "thing",
    "things",
    "stuff",
    "step",
    "help",
    "like",
    "look",
];

/// Kinds that can be recall results (topics, owners and people only carry activation).
const RESULT_KINDS: &[&str] = &[
    "concept", "repo", "project", "site", "post", "dataset", "stage",
];

/// Normalized search terms: lowercase words, stop words removed, light stemming.
#[must_use]
pub fn terms(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !STOP.contains(w))
        .map(stem)
        .collect()
}

/// Strip plural/-ing/-ed endings. Deliberately not `-er`: "engineer" must not become "engine".
fn stem(word: &str) -> String {
    ["ings", "ing", "ed", "es", "s"]
        .iter()
        .find_map(|suffix| word.strip_suffix(suffix).filter(|base| base.len() >= 4))
        .unwrap_or(word)
        .to_owned()
}

fn kind_weight(kind: &str) -> f32 {
    match kind {
        "concept" => 1.5,
        "project" | "stage" => 1.2,
        "repo" | "site" => 1.0,
        "dataset" => 0.9,
        "post" => 0.8,
        "topic" => 0.7,
        "account" => 0.4,
        _ => 0.15,
    }
}

/// How strongly activation flows along a relation, in either direction.
fn relation_weight(relation: &str) -> f32 {
    match relation {
        "provides" | "implements" => 0.8,
        "used_at_stage" => 0.7,
        "needs" | "part_of" => 0.6,
        "alternative_to" | "complements" | "builds_on" | "tagged" => 0.5,
        "instance_of" | "mentions" => 0.45,
        "relevant_to" | "used_in" | "uses" => 0.4,
        "links_to" | "listed_in" | "precedes" => 0.3,
        _ => 0.25,
    }
}

/// One ranked result.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: AnchorId,
    pub score: f32,
    /// Share of the score from matching the question directly (0 = reached only via links).
    pub direct: f32,
    /// Why this item is in the package.
    pub why: String,
}

#[derive(Debug, Clone)]
pub struct RecallResult {
    pub query: String,
    pub terms: Vec<String>,
    pub items: Vec<Item>,
}

/// Rank the memory's anchors for `query`, best first, at most `max_items`.
#[must_use]
pub fn recall(state: &State, query: &str, max_items: usize) -> RecallResult {
    let query_terms: Vec<String> = {
        let mut seen = HashSet::new();
        terms(query)
            .into_iter()
            .filter(|t| seen.insert(t.clone()))
            .collect()
    };
    let direct = recognize(state, &query_terms);
    let (activation, why) = activate(state, &direct);

    let max = direct
        .values()
        .map(|(s, _)| *s)
        .fold(0.0_f32, f32::max)
        .max(1e-6);
    let mut items: Vec<Item> = activation
        .into_iter()
        .filter(|(id, _)| RESULT_KINDS.contains(&id.kind()))
        .map(|(id, score)| Item {
            direct: direct.get(&id).map_or(0.0, |(s, _)| s / max),
            why: why.get(&id).cloned().unwrap_or_default(),
            id,
            score,
        })
        .collect();
    items.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    items.truncate(max_items);
    RecallResult {
        query: query.to_owned(),
        terms: query_terms,
        items,
    }
}

/// Text that describes an anchor: name and aliases (weighted ×3), summary, meaning claims.
fn document(state: &State, id: &AnchorId) -> Vec<String> {
    let anchor = &state.anchors[id].anchor;
    let mut name = terms(&anchor.name);
    name.extend(terms(id.as_str()));
    for alias in &anchor.aliases {
        name.extend(terms(alias));
    }
    let mut doc = Vec::with_capacity(name.len() * 3);
    for _ in 0..3 {
        doc.extend(name.iter().cloned());
    }
    doc.extend(terms(anchor.summary.as_deref().unwrap_or("")));
    for cs in state.about(id.as_str()) {
        if let Value::Text(text) = &cs.claim.value {
            use organon_protocol::Family as F;
            let family = cs.claim.relation.info().family;
            if matches!(family, F::Meaning | F::Bounds | F::Frontier | F::Other)
                || cs.claim.relation.as_str() == "described_as"
            {
                doc.extend(terms(text));
            }
        }
    }
    doc
}

/// BM25 score of every present anchor that matches, with the terms it matched.
#[allow(clippy::cast_precision_loss)]
fn recognize(state: &State, query: &[String]) -> HashMap<AnchorId, (f32, Vec<String>)> {
    let ids: Vec<&AnchorId> = state
        .anchors
        .keys()
        .filter(|id| state.is_present(id.as_str()))
        .collect();
    let docs: Vec<Vec<String>> = ids.iter().map(|id| document(state, id)).collect();
    let n = docs.len().max(1) as f32;
    let avg_len = docs.iter().map(Vec::len).sum::<usize>() as f32 / n;
    let mut df: HashMap<&str, f32> = HashMap::new();
    for doc in &docs {
        for t in doc.iter().map(String::as_str).collect::<HashSet<_>>() {
            *df.entry(t).or_default() += 1.0;
        }
    }
    let (k1, b) = (1.2_f32, 0.75_f32);
    let mut out = HashMap::new();
    for (id, doc) in ids.iter().zip(&docs) {
        let mut score = 0.0;
        let mut hits = Vec::new();
        for t in query {
            let tf = doc.iter().filter(|x| *x == t).count() as f32;
            if tf == 0.0 {
                continue;
            }
            let d = df[t.as_str()];
            let idf = ((n - d + 0.5) / (d + 0.5) + 1.0).ln();
            score += idf * tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * doc.len() as f32 / avg_len));
            hits.push(t.clone());
        }
        if score > 0.0 {
            out.insert((*id).clone(), (score * kind_weight(id.kind()), hits));
        }
    }
    out
}

/// Spread activation two hops along meaningful anchor-to-anchor links.
fn activate(
    state: &State,
    direct: &HashMap<AnchorId, (f32, Vec<String>)>,
) -> (HashMap<AnchorId, f32>, HashMap<AnchorId, String>) {
    let max = direct
        .values()
        .map(|(s, _)| *s)
        .fold(0.0_f32, f32::max)
        .max(1e-6);
    let mut activation: HashMap<AnchorId, f32> = direct
        .iter()
        .map(|(k, (s, _))| (k.clone(), s / max))
        .collect();
    let mut why: HashMap<AnchorId, String> = direct
        .iter()
        .map(|(k, (_, hits))| (k.clone(), format!("matches \"{}\"", hits.join("\", \""))))
        .collect();
    let edges: Vec<(&AnchorId, &AnchorId, &str, f32)> = state
        .active()
        .filter_map(|cs| {
            let c = &cs.claim;
            let from = c.about.anchor()?;
            let to = c.value.anchor()?;
            let family = c.relation.info().family;
            let keep = !matches!(
                family,
                organon_protocol::Family::Meta | organon_protocol::Family::Personal
            );
            keep.then(|| {
                (
                    from,
                    to,
                    c.relation.as_str(),
                    relation_weight(c.relation.as_str()) * c.confidence.unwrap_or(0.9) * 0.5,
                )
            })
        })
        .collect();

    for _hop in 0..2 {
        // per target: (total gain, best single gain, reason for the best path)
        let mut gains: HashMap<&AnchorId, (f32, f32, String)> = HashMap::new();
        for &(from, to, relation, weight) in &edges {
            for (src, dst, forward) in [(from, to, true), (to, from, false)] {
                let Some(&a) = activation.get(src) else {
                    continue;
                };
                if a < 0.05 {
                    continue;
                }
                let gain = a * weight;
                let entry = gains.entry(dst).or_insert((0.0, 0.0, String::new()));
                if gain > entry.1 {
                    let name = display_name(state, src.as_str());
                    entry.1 = gain;
                    entry.2 = if forward {
                        format!("{name} --{relation}--> this")
                    } else {
                        format!("this --{relation}--> {name}")
                    };
                }
                entry.0 += gain;
            }
        }
        for (target, (total, best, reason)) in gains {
            let gain = best + 0.3 * (total - best).sqrt();
            let entry = activation.entry(target.clone()).or_insert(0.0);
            if *entry == 0.0 {
                why.insert(target.clone(), reason);
            }
            *entry += gain;
        }
    }
    (activation, why)
}

fn display_name(state: &State, id: &str) -> String {
    state
        .anchor(id)
        .map_or_else(|| id.to_owned(), |a| a.anchor.name.clone())
}

/// A rendered package and what went into it.
#[derive(Debug, Clone)]
pub struct Package {
    pub markdown: String,
    pub included: Vec<AnchorId>,
    pub omitted: Vec<AnchorId>,
}

struct RenderedItem {
    text: String,
    unknown: Vec<String>,
}

fn render_item(state: &State, frames: &Frames, item: &Item, rank: usize) -> RenderedItem {
    let anchor = &state.anchors[&item.id].anchor;
    let fills = frames
        .for_kind(anchor.kind())
        .map(|f| frames::profile(f, state, item.id.as_str()));
    let coverage = fills.as_deref().map_or(0.0, frames::coverage);
    let mut text = String::new();
    let _ = writeln!(
        text,
        "### {rank}. {} ({}) · relevance {:.2} · understood {:.0}%",
        anchor.name,
        anchor.kind(),
        item.score,
        coverage * 100.0
    );
    let _ = writeln!(text, "why here: {}", item.why);
    if let Some(summary) = &anchor.summary {
        let _ = writeln!(text, "> {}", summary.replace('\n', " "));
    }
    let mut unknown = Vec::new();
    for fill in fills.iter().flatten() {
        if fill.is_empty() {
            unknown.push(fill.slot.label.clone());
            continue;
        }
        if fill.slot.id == "purpose" && anchor.summary.is_some() && fill.claims.len() == 1 {
            continue; // the summary already shows it
        }
        let values: Vec<String> = fill
            .claims
            .iter()
            .take(8)
            .map(|cs| {
                let c = &cs.claim;
                let shown = if c.about.as_str() == item.id.as_str() {
                    match &c.value {
                        Value::Anchor(x) => display_name(state, x.as_str()),
                        Value::Number(_) if c.relation.as_str() == "stars" => {
                            format!("{}★", c.value)
                        }
                        v => v.to_string(),
                    }
                } else {
                    display_name(state, c.about.as_str())
                };
                let conf = c.confidence.map(|x| format!(" {x:.2}")).unwrap_or_default();
                format!("{shown} [{}{conf}]", c.basis.as_str())
            })
            .collect();
        let more = fill
            .claims
            .len()
            .checked_sub(8)
            .filter(|m| *m > 0)
            .map(|m| format!(" (+{m} more)"))
            .unwrap_or_default();
        let _ = writeln!(text, "- {}: {}{more}", fill.slot.label, values.join("; "));
    }
    if !unknown.is_empty() {
        let _ = writeln!(text, "- NOT KNOWN: {}", unknown.join(", "));
    }
    RenderedItem { text, unknown }
}

/// Full package within `budget_tokens` (~4 characters per token).
#[must_use]
pub fn render(
    state: &State,
    frames: &Frames,
    result: &RecallResult,
    head: &str,
    budget_tokens: usize,
) -> Package {
    let budget_chars = budget_tokens * 4;
    let mut md = String::new();
    let _ = writeln!(
        md,
        "# Context package\n\nquestion: {}\nterms: {}\nmemory: {head} · {} anchors · {} active claims\n",
        result.query,
        result.terms.join(", "),
        state.anchors.len(),
        state.active().count()
    );
    md.push_str("Bases: observed = read from a source by a tool; stated = said by an author or Philip; inferred = reasoned (with confidence). \"NOT KNOWN\" = empty frame slot. Do not fill it by guessing; say it is unknown.\n\n");
    let (mut included, mut omitted) = (Vec::new(), Vec::new());
    let mut gaps: HashMap<String, usize> = HashMap::new();
    for item in &result.items {
        let rendered = render_item(state, frames, item, included.len() + 1);
        if md.len() + rendered.text.len() > budget_chars && !included.is_empty() {
            omitted.push(item.id.clone());
            continue;
        }
        for gap in rendered.unknown {
            *gaps.entry(gap).or_default() += 1;
        }
        md.push_str(&rendered.text);
        md.push('\n');
        included.push(item.id.clone());
    }
    if result.items.is_empty() {
        md.push_str("Nothing in memory matches this question. That is itself the answer: this area is unexplored.\n");
    }
    let mut gaps: Vec<(String, usize)> = gaps.into_iter().collect();
    gaps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if !gaps.is_empty() {
        let listed: Vec<String> = gaps
            .iter()
            .take(6)
            .map(|(k, v)| format!("{k} (missing for {v} of {} items)", included.len()))
            .collect();
        let _ = writeln!(
            md,
            "## What the memory does not know here\n{}\n",
            listed.join(" · ")
        );
    }
    if !omitted.is_empty() {
        let ids: Vec<&str> = omitted.iter().map(AnchorId::as_str).collect();
        let _ = writeln!(
            md,
            "## Omitted for budget ({}): fetch with `mneme page <id>`\n{}\n",
            omitted.len(),
            ids.join(", ")
        );
    }
    Package {
        markdown: md,
        included,
        omitted,
    }
}

/// One line per item (~25 tokens each): the cheapest useful view. Fetch detail with
/// `mneme page <id>` or a full recall only where needed.
#[must_use]
pub fn render_brief(state: &State, frames: &Frames, result: &RecallResult, head: &str) -> Package {
    let mut md = format!("recall: {} · memory {head}\n", result.query);
    for item in &result.items {
        let anchor = &state.anchors[&item.id].anchor;
        let fills = frames
            .for_kind(anchor.kind())
            .map(|f| frames::profile(f, state, item.id.as_str()));
        let understood = fills.as_deref().map_or_else(
            || "—".to_owned(),
            |f| format!("{:.0}%", frames::coverage(f) * 100.0),
        );
        let gaps: Vec<&str> = fills
            .iter()
            .flatten()
            .filter(|f| f.is_empty())
            .map(|f| f.slot.id.as_str())
            .collect();
        let summary: String = anchor
            .summary
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(90)
            .collect();
        let gaps = if gaps.is_empty() {
            "—".to_owned()
        } else {
            gaps.join(",")
        };
        let _ = writeln!(
            md,
            "- {} [{}] {understood} · {} · unknown: {gaps}",
            item.id,
            anchor.kind(),
            summary.replace('\n', " ")
        );
    }
    if result.items.is_empty() {
        md.push_str("nothing in memory matches: unexplored area\n");
    }
    Package {
        markdown: md,
        included: result.items.iter().map(|i| i.id.clone()).collect(),
        omitted: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stemming_keeps_engineer_apart_from_engine() {
        assert_eq!(terms("reverse engineering"), ["reverse", "engineer"]);
        assert_ne!(stem("engineer"), stem("engine"));
        assert_eq!(stem("designs"), "design");
    }

    #[test]
    fn stop_words_and_single_letters_are_dropped() {
        assert_eq!(terms("How do I make a website?"), ["website"]);
    }
}
