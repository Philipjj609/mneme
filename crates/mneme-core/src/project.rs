//! Projections: regenerable views of the log. Never authoritative; delete and rebuild freely.
//!
//! ```text
//! projections/graph.json   time-indexed data (anchors, claims, commits, frames)
//! projections/view.html    the explorer, data inlined (opens from file://)
//! projections/wiki/        pages: index.md, log.md, <kind>/<name>.md
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use organon_protocol::Family;
use serde_json::{json, Value as J};

use crate::error::{Error, Result};
use crate::frames::{self, Frames};
use crate::model::{Commit, CommitId, Op, Stance, Value};
use crate::state::{ClaimState, State};
use crate::util::{now_rfc3339, path_parts};

const VIEWER: &str = include_str!("viewer.html");

/// Branch context shown in the views.
#[derive(Debug, Clone, Copy)]
pub struct BuildInfo<'a> {
    pub branch: &'a str,
    pub branches: &'a BTreeMap<String, CommitId>,
}

/// The data the viewer renders. **Format consumed by `viewer.html`**: keep keys stable.
#[must_use]
pub fn data_json(lineage: &[Commit], state: &State, frames: &Frames, info: BuildInfo) -> J {
    let commits: Vec<J> = lineage
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let count = |pred: fn(&Op) -> bool| c.ops.iter().filter(|op| pred(op)).count();
            json!({ "idx": i, "id": c.id, "author": c.author, "message": c.message, "time": c.time,
                    "added": { "anchors": count(|o| matches!(o, Op::AddAnchor { .. })),
                               "claims": count(|o| matches!(o, Op::AddClaim { .. })),
                               "sources": count(|o| matches!(o, Op::AddSource { .. })),
                               "status": count(|o| matches!(o, Op::SetStatus { .. })) } })
        })
        .collect();
    let anchors: Vec<J> = state
        .anchors
        .values()
        .map(|a| {
            json!({ "id": a.anchor.id, "kind": a.anchor.kind(), "name": a.anchor.name, "scope": a.anchor.scope,
                    "summary": a.anchor.summary, "aliases": a.anchor.aliases, "introduced": a.introduced })
        })
        .collect();
    let claims: Vec<J> = state.claims.values().map(claim_json).collect();
    let sources: Vec<J> = state
        .sources
        .values()
        .map(|s| {
            json!({ "id": s.source.id, "kind": s.source.kind, "uri": s.source.uri, "title": s.source.title,
                    "captured_at": s.source.captured_at, "introduced": s.introduced })
        })
        .collect();
    let families: Vec<&str> = Family::ALL.iter().map(|f| f.as_str()).collect();
    json!({
        "meta": { "built_at": now_rfc3339(), "branch": info.branch, "head": lineage.last().map(|c| &c.id),
                  "branches": info.branches, "families": families },
        "commits": commits, "frames": frames.as_slice(), "anchors": anchors, "claims": claims, "sources": sources
    })
}

fn claim_json(cs: &ClaimState) -> J {
    let c = &cs.claim;
    let (vk, v) = match &c.value {
        Value::Anchor(a) => ("anchor", json!(a)),
        Value::Text(t) => ("text", json!(t)),
        Value::Number(n) => ("number", json!(n)),
        Value::Bool(b) => ("bool", json!(b)),
    };
    let evidence: Vec<_> = c.evidence_sources().collect();
    json!({ "id": c.id, "about": c.about, "relation": c.relation, "family": c.relation.info().family.as_str(),
            "vk": vk, "v": v, "scope": c.scope, "basis": c.basis.as_str(), "stance": c.stance.as_str(),
            "confidence": c.confidence, "rationale": c.rationale, "valid_from": c.valid_from, "author": c.author,
            "evidence": evidence, "status": cs.status.as_str(), "introduced": cs.introduced, "changed": cs.changed,
            "reinforced": cs.reinforced, "replaced_by": cs.replaced_by })
}

/// Write every projection into `out`.
///
/// # Errors
/// I/O failures.
pub fn write_all(
    out: &Path,
    lineage: &[Commit],
    state: &State,
    frames: &Frames,
    info: BuildInfo,
) -> Result<()> {
    fs::create_dir_all(out).map_err(Error::io(out))?;
    let data = data_json(lineage, state, frames, info);
    let text = serde_json::to_string(&data).map_err(|source| Error::Json {
        path: out.join("graph.json"),
        source,
    })?;
    write(&out.join("graph.json"), &text)?;
    // Inline the data so the page works from file:// with no server; `</` must not close <script>.
    write(
        &out.join("view.html"),
        &VIEWER.replace("/*MNEME_DATA*/null", &text.replace("</", "<\\/")),
    )?;
    write_wiki(&out.join("wiki"), lineage, state, frames)
}

fn write(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).map_err(Error::io(path))
}

fn page_link(from_kind_dir: bool, id: &str) -> String {
    let (kind, name) = path_parts(id);
    if from_kind_dir {
        format!("../{kind}/{name}.md")
    } else {
        format!("{kind}/{name}.md")
    }
}

fn display_name(state: &State, id: &str) -> String {
    state
        .anchor(id)
        .map_or_else(|| id.to_owned(), |a| a.anchor.name.clone())
}

fn render_claim(state: &State, cs: &ClaimState, incoming: bool) -> String {
    let c = &cs.claim;
    let target = if incoming {
        c.about.anchor().map(|a| a.as_str())
    } else {
        c.value.anchor().map(|a| a.as_str())
    };
    let mut s = match target {
        Some(t) => format!("[{}]({})", display_name(state, t), page_link(true, t)),
        None => c.value.to_string().replace('\n', " "),
    };
    let _ = write!(s, " — *{}*", c.basis.as_str());
    if c.stance != Stance::Is {
        let _ = write!(s, ", {}", c.stance.as_str());
    }
    if let Some(conf) = c.confidence {
        let _ = write!(s, ", conf {conf:.2}");
    }
    if let Some(v) = &c.valid_from {
        let _ = write!(s, ", since {}", v.get(..10).unwrap_or(v));
    }
    if cs.reinforced > 0 {
        let _ = write!(s, ", reinforced ×{}", cs.reinforced);
    }
    if let Some(r) = &c.rationale {
        let _ = write!(s, "<br>  ↳ {}", r.replace('\n', " "));
    }
    s
}

/// (name, id, coverage) of one page in the wiki index.
type IndexEntry = (String, String, Option<f32>);

fn write_wiki(dir: &Path, lineage: &[Commit], state: &State, frames: &Frames) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(Error::io(dir))?;
    }
    fs::create_dir_all(dir).map_err(Error::io(dir))?;
    let mut by_kind: BTreeMap<&str, Vec<IndexEntry>> = BTreeMap::new();

    for a in state
        .anchors
        .values()
        .filter(|a| state.is_present(a.anchor.id.as_str()))
    {
        let id = a.anchor.id.as_str();
        let (kind_dir, file) = path_parts(id);
        fs::create_dir_all(dir.join(&kind_dir)).map_err(Error::io(dir.join(&kind_dir)))?;
        let mut md = format!(
            "# {}\n\n`{id}` · kind **{}** · scope `{}` · known since commit {}\n\n",
            a.anchor.name,
            a.anchor.kind(),
            a.anchor.scope,
            a.introduced
        );
        if let Some(s) = &a.anchor.summary {
            let _ = writeln!(md, "> {}\n", s.replace('\n', " "));
        }
        let mut shown: Vec<&str> = Vec::new();
        let mut coverage = None;
        if let Some(frame) = frames.for_kind(a.anchor.kind()) {
            let fills = frames::profile(frame, state, id);
            let c = frames::coverage(&fills);
            coverage = Some(c);
            let _ = writeln!(
                md,
                "## Understanding ({:.0}% of the {} frame)\n",
                c * 100.0,
                frame.label
            );
            for fill in fills.iter().filter(|f| !f.is_empty()) {
                let _ = writeln!(md, "**{}**", fill.slot.label);
                for cs in &fill.claims {
                    shown.push(cs.claim.id.as_str());
                    let _ = writeln!(
                        md,
                        "- {}",
                        render_claim(state, cs, cs.claim.about.as_str() != id)
                    );
                }
                md.push('\n');
            }
            let frontier: Vec<_> = fills.iter().filter(|f| f.is_empty()).collect();
            if !frontier.is_empty() {
                md.push_str("## Frontier — not yet understood\n\n");
                for f in frontier {
                    let hint = f
                        .slot
                        .hint
                        .as_deref()
                        .map(|h| format!(" — {h}"))
                        .unwrap_or_default();
                    let _ = writeln!(md, "- [ ] **{}**{hint}", f.slot.label);
                }
                md.push('\n');
            }
        }
        let other: Vec<_> = state
            .about(id)
            .filter(|cs| !shown.contains(&cs.claim.id.as_str()))
            .collect();
        if !other.is_empty() {
            md.push_str("## Other claims\n\n");
            for cs in other {
                let _ = writeln!(
                    md,
                    "- `{}` {}",
                    cs.claim.relation,
                    render_claim(state, cs, false)
                );
            }
            md.push('\n');
        }
        let incoming: Vec<_> = state
            .pointing_to(id)
            .filter(|cs| !shown.contains(&cs.claim.id.as_str()))
            .collect();
        if !incoming.is_empty() {
            md.push_str("## Linked from\n\n");
            for cs in incoming {
                let _ = writeln!(
                    md,
                    "- `{}` ← {}",
                    cs.claim.relation,
                    render_claim(state, cs, true)
                );
            }
            md.push('\n');
        }
        let mut sources: Vec<_> = state
            .about(id)
            .flat_map(|cs| cs.claim.evidence_sources())
            .collect();
        sources.sort();
        sources.dedup();
        if !sources.is_empty() {
            md.push_str("## Evidence\n\n");
            for s in sources.into_iter().filter_map(|s| state.sources.get(s)) {
                let date = s
                    .source
                    .captured_at
                    .get(..10)
                    .unwrap_or(&s.source.captured_at);
                let _ = writeln!(
                    md,
                    "- `{}` {} — {} ({date})",
                    s.source.id, s.source.kind, s.source.uri
                );
            }
        }
        write(&dir.join(&kind_dir).join(format!("{file}.md")), &md)?;
        by_kind.entry(a.anchor.kind()).or_default().push((
            a.anchor.name.clone(),
            id.to_owned(),
            coverage,
        ));
    }

    let mut index =
        String::from("# mneme — index\n\nGenerated projection of the memory log. Do not edit; rebuild with `mneme build`.\n\n");
    for (kind, mut items) in by_kind {
        items.sort_by_key(|(name, _, _)| name.to_lowercase());
        let _ = writeln!(index, "## {kind} ({})\n", items.len());
        for (name, id, coverage) in items {
            let c = coverage
                .map(|c| format!(" · {:.0}% understood", c * 100.0))
                .unwrap_or_default();
            let _ = writeln!(index, "- [{name}]({}){c}", page_link(false, &id));
        }
        index.push('\n');
    }
    write(&dir.join("index.md"), &index)?;

    let mut log = String::from("# mneme — log\n\n");
    for (i, c) in lineage.iter().enumerate().rev() {
        let date = c.time.get(..10).unwrap_or(&c.time);
        let _ = writeln!(
            log,
            "- **{i}** `{}` {date} — {} ({} ops) — *{}*",
            c.id,
            c.message,
            c.ops.len(),
            c.author
        );
    }
    write(&dir.join("log.md"), &log)
}
