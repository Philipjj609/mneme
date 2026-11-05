//! Recall: compile a context package for a question.
//!
//! recognize (BM25 over anchor text) → activate (spread along meaningful links, weighted by
//! relation and confidence) → package (ranked items with *why they are here*, what is known,
//! what is NOT known, what was left out and how to fetch it).
//!
//! No model is needed to recall. A model reads the package and answers from it.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::frames::{self, Frame};
use crate::model::*;
use crate::relations;
use crate::state::State;

const STOP: &[&str] = &[
    "a", "an", "the", "of", "to", "and", "or", "for", "in", "on", "at", "by", "with", "about", "from",
    "how", "what", "which", "who", "why", "when", "where", "is", "are", "be", "do", "does", "can",
    "could", "should", "would", "will", "i", "me", "my", "we", "our", "you", "your", "it", "this",
    "that", "these", "those", "based", "explain", "tell", "use", "using", "best", "way", "ways",
    "make", "give", "show", "know", "want", "need", "some", "any", "all", "get", "good", "into",
    "resource", "resources", "tool", "tools", "thing", "things", "stuff", "step", "help", "like", "look",
];

pub fn terms(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !STOP.contains(w))
        .map(stem)
        .collect()
}

fn stem(w: &str) -> String {
    for suf in ["ings", "ing", "ed", "es", "s"] {
        if let Some(base) = w.strip_suffix(suf) {
            if base.len() >= 4 {
                return base.to_string();
            }
        }
    }
    w.to_string()
}

fn kind_weight(kind: &str) -> f32 {
    match kind {
        "concept" => 1.5,
        "project" => 1.2,
        "stage" => 1.2,
        "repo" | "site" => 1.0,
        "dataset" => 0.9,
        "post" => 0.8,
        "topic" => 0.7,
        "account" => 0.4,
        _ => 0.15,
    }
}

/// How strongly activation flows along a relation (either direction).
fn rel_weight(rel: &str) -> f32 {
    match rel {
        "provides" | "implements" => 0.8,
        "used_at_stage" => 0.7,
        "instance_of" | "mentions" => 0.45,
        "links_to" | "listed_in" | "precedes" => 0.3,
        "needs" | "part_of" => 0.6,
        "alternative_to" | "complements" | "builds_on" | "tagged" => 0.5,
        "relevant_to" | "used_in" | "uses" => 0.4,
        _ => 0.25,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: Id,
    pub score: f32,
    pub direct: f32,
    /// Human-readable reason this item is in the package.
    pub why: String,
}

pub struct RecallResult {
    pub query: String,
    pub terms: Vec<String>,
    pub items: Vec<Item>,
}

fn anchor_doc(state: &State, id: &str) -> (Vec<String>, Vec<String>) {
    let a = &state.anchors[id].anchor;
    let mut name = terms(&a.name);
    name.extend(terms(&a.id));
    for al in &a.aliases {
        name.extend(terms(al));
    }
    let mut body = terms(a.summary.as_deref().unwrap_or(""));
    for cs in state.about(id) {
        if let Value::Text(t) = &cs.claim.value {
            let fam = relations::info(&cs.claim.relation).family;
            if matches!(fam, "meaning" | "bounds" | "frontier" | "other") || cs.claim.relation == "described_as" {
                body.extend(terms(t));
            }
        }
    }
    (name, body)
}

pub fn recall(state: &State, query: &str, max_items: usize) -> RecallResult {
    let q: Vec<String> = {
        let mut seen = HashSet::new();
        terms(query).into_iter().filter(|t| seen.insert(t.clone())).collect()
    };

    // --- recognize: BM25 over anchors (name field counts triple) ---
    // Only things something is still said about (retracted-only anchors drop out).
    let ids: Vec<&Id> = state
        .anchors
        .keys()
        .filter(|id| state.about(id).next().is_some() || state.pointing_to(id).next().is_some())
        .collect();
    let docs: Vec<Vec<String>> = ids
        .iter()
        .map(|id| {
            let (name, body) = anchor_doc(state, id);
            let mut d = Vec::with_capacity(name.len() * 3 + body.len());
            for _ in 0..3 {
                d.extend(name.iter().cloned());
            }
            d.extend(body);
            d
        })
        .collect();
    let n = docs.len().max(1) as f32;
    let avg = docs.iter().map(|d| d.len()).sum::<usize>() as f32 / n;
    let mut df: HashMap<&str, f32> = HashMap::new();
    for d in &docs {
        let uniq: HashSet<&str> = d.iter().map(|s| s.as_str()).collect();
        for t in uniq {
            *df.entry(t).or_default() += 1.0;
        }
    }
    let (k1, b) = (1.2f32, 0.75f32);
    let mut direct: HashMap<Id, (f32, Vec<String>)> = HashMap::new();
    for (i, d) in docs.iter().enumerate() {
        let mut s = 0.0;
        let mut hit = vec![];
        for t in &q {
            let tf = d.iter().filter(|x| *x == t).count() as f32;
            if tf == 0.0 {
                continue;
            }
            let idf = ((n - df[t.as_str()] + 0.5) / (df[t.as_str()] + 0.5) + 1.0).ln();
            s += idf * tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * d.len() as f32 / avg));
            hit.push(t.clone());
        }
        if s > 0.0 {
            let kind = &state.anchors[ids[i]].anchor.kind;
            direct.insert(ids[i].clone(), (s * kind_weight(kind), hit));
        }
    }
    let max = direct.values().map(|v| v.0).fold(0.0f32, f32::max).max(1e-6);

    // --- activate: two hops of spreading activation along meaningful links ---
    let mut act: HashMap<Id, f32> = direct.iter().map(|(k, v)| (k.clone(), v.0 / max)).collect();
    let mut why: HashMap<Id, String> = direct
        .iter()
        .map(|(k, v)| (k.clone(), format!("matches \"{}\"", v.1.join("\", \""))))
        .collect();
    let edges: Vec<&crate::state::ClaimState> = state
        .active()
        .filter(|cs| {
            cs.claim.value.anchor().is_some()
                && !matches!(relations::info(&cs.claim.relation).family, "meta" | "personal")
        })
        .collect();
    for _hop in 0..2 {
        // (total gain, best single gain, reason for the best path)
        let mut add: HashMap<Id, (f32, f32, String)> = HashMap::new();
        for cs in &edges {
            let c = &cs.claim;
            let to = c.value.anchor().unwrap();
            let w = rel_weight(&c.relation) * c.confidence.unwrap_or(0.9) * 0.5;
            // forward: about --rel--> to ; backward: to <--rel-- about
            for (from, dst, forward) in [(c.about.as_str(), to, true), (to, c.about.as_str(), false)] {
                let Some(&a) = act.get(from) else { continue };
                if a < 0.05 {
                    continue;
                }
                let gain = a * w;
                let e = add.entry(dst.to_string()).or_insert((0.0, 0.0, String::new()));
                if gain > e.1 {
                    let fname = name(state, from);
                    e.1 = gain;
                    e.2 = if forward {
                        format!("{fname} --{}--> this", c.relation)
                    } else {
                        format!("this --{}--> {fname}", c.relation)
                    };
                }
                e.0 += gain;
            }
        }
        for (k, (g, _, reason)) in add {
            let entry = act.entry(k.clone()).or_insert(0.0);
            if *entry == 0.0 {
                why.insert(k.clone(), reason);
            }
            *entry += g;
        }
    }

    let mut items: Vec<Item> = act
        .into_iter()
        .filter(|(id, _)| {
            let k = state.anchors.get(id).map(|a| a.anchor.kind.as_str()).unwrap_or("");
            matches!(k, "concept" | "repo" | "project" | "site" | "post" | "dataset" | "stage")
        })
        .map(|(id, score)| Item {
            direct: direct.get(&id).map(|d| d.0 / max).unwrap_or(0.0),
            why: why.get(&id).cloned().unwrap_or_default(),
            id,
            score,
        })
        .collect();
    items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap().then(a.id.cmp(&b.id)));
    items.truncate(max_items);
    RecallResult { query: query.into(), terms: q, items }
}

fn name(state: &State, id: &str) -> String {
    state.anchors.get(id).map(|a| a.anchor.name.clone()).unwrap_or_else(|| id.into())
}

/// One item rendered compactly: what it is, what is known slot by slot, what is not.
fn render_item(state: &State, frames: &[Frame], it: &Item, rank: usize) -> String {
    let a = &state.anchors[&it.id].anchor;
    let mut s = String::new();
    let frame = frames.iter().find(|f| f.kind == a.kind);
    let fills = frame.map(|f| frames::profile(f, state, &a.id));
    let cov = fills.as_ref().map(|f| frames::coverage(f)).unwrap_or(0.0);
    let _ = writeln!(s, "### {rank}. {} ({}) · relevance {:.2} · understood {:.0}%", a.name, a.kind, it.score, cov * 100.0);
    let _ = writeln!(s, "why here: {}", it.why);
    if let Some(sum) = &a.summary {
        let _ = writeln!(s, "> {}", sum.replace('\n', " "));
    }
    if let Some(fills) = fills {
        let mut unknown = vec![];
        for f in &fills {
            if f.claims.is_empty() {
                unknown.push(f.slot.label.as_str());
                continue;
            }
            if f.slot.id == "purpose" && a.summary.is_some() && f.claims.len() == 1 {
                continue; // summary already shows it
            }
            let vals: Vec<String> = f
                .claims
                .iter()
                .take(8)
                .map(|cs| {
                    let c = &cs.claim;
                    let v = if c.about != a.id {
                        name(state, &c.about)
                    } else {
                        match &c.value {
                            Value::Anchor(x) => name(state, x),
                            Value::Number(_) if c.relation == "stars" => format!("{}★", c.value.display()),
                            v => v.display(),
                        }
                    };
                    let conf = c.confidence.map(|x| format!(" {x:.2}")).unwrap_or_default();
                    format!("{v} [{}{conf}]", c.basis.as_str())
                })
                .collect();
            let more = if f.claims.len() > 8 { format!(" (+{} more)", f.claims.len() - 8) } else { String::new() };
            let _ = writeln!(s, "- {}: {}{more}", f.slot.label, vals.join("; "));
        }
        if !unknown.is_empty() {
            let _ = writeln!(s, "- NOT KNOWN: {}", unknown.join(", "));
        }
    }
    s
}

/// One line per item: the cheapest useful view (~25 tokens each). Use first; fetch detail with
/// `mneme page <id>` or a full recall only where needed (progressive disclosure).
pub fn render_brief(state: &State, frames: &[Frame], r: &RecallResult, head: &str) -> Package {
    let mut md = format!("recall: {} · memory {head}\n", r.query);
    let mut included = vec![];
    for it in &r.items {
        let a = &state.anchors[&it.id].anchor;
        let fills = frames.iter().find(|f| f.kind == a.kind).map(|f| frames::profile(f, state, &a.id));
        let cov = fills.as_ref().map(|f| frames::coverage(f)).unwrap_or(0.0);
        let gaps: Vec<&str> = fills.as_ref().map(|f| f.iter().filter(|x| x.claims.is_empty()).map(|x| x.slot.id.as_str()).collect()).unwrap_or_default();
        let summary: String = a.summary.as_deref().unwrap_or("").chars().take(90).collect();
        let understood = if fills.is_some() { format!("{:.0}%", cov * 100.0) } else { "—".into() };
        let _ = writeln!(md, "- {} [{}] {} · {} · unknown: {}", a.id, a.kind, understood, summary.replace('\n', " "),
            if gaps.is_empty() { "—".to_string() } else { gaps.join(",") });
        included.push(it.id.clone());
    }
    if r.items.is_empty() {
        md.push_str("nothing in memory matches: unexplored area\n");
    }
    Package { markdown: md, included, omitted: vec![] }
}

pub struct Package {
    pub markdown: String,
    pub included: Vec<Id>,
    pub omitted: Vec<Id>,
}

pub fn render(state: &State, frames: &[Frame], r: &RecallResult, head: &str, budget_tokens: usize) -> Package {
    let budget_chars = budget_tokens * 4;
    let mut md = String::new();
    let _ = writeln!(md, "# Context package\n\nquestion: {}\nterms: {}\nmemory: {head} · {} anchors · {} active claims\n",
        r.query, r.terms.join(", "), state.anchors.len(), state.active().count());
    let _ = writeln!(md, "Bases: observed = read from a source by a tool; stated = said by an author or Philip; inferred = reasoned (with confidence). \"NOT KNOWN\" = empty frame slot. Do not fill it by guessing; say it is unknown.\n");
    let mut included = vec![];
    let mut omitted = vec![];
    let mut gaps: HashMap<String, usize> = HashMap::new();
    for it in &r.items {
        let block = render_item(state, frames, it, included.len() + 1);
        if md.len() + block.len() > budget_chars && !included.is_empty() {
            omitted.push(it.id.clone());
            continue;
        }
        for line in block.lines().filter(|l| l.starts_with("- NOT KNOWN: ")) {
            for g in line.trim_start_matches("- NOT KNOWN: ").split(", ") {
                *gaps.entry(g.to_string()).or_default() += 1;
            }
        }
        md.push_str(&block);
        md.push('\n');
        included.push(it.id.clone());
    }
    if r.items.is_empty() {
        md.push_str("Nothing in memory matches this question. That is itself the answer: this area is unexplored.\n");
    }
    let mut g: Vec<_> = gaps.into_iter().collect();
    g.sort_by(|a, b| b.1.cmp(&a.1));
    if !g.is_empty() {
        let _ = writeln!(md, "## What the memory does not know here\n{}\n",
            g.iter().take(6).map(|(k, v)| format!("{k} (missing for {v} of {} items)", included.len())).collect::<Vec<_>>().join(" · "));
    }
    if !omitted.is_empty() {
        let _ = writeln!(md, "## Omitted for budget ({}) — fetch with `mneme page <id>`\n{}\n", omitted.len(), omitted.join(", "));
    }
    Package { markdown: md, included, omitted }
}
