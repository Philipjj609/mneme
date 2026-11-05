//! Projections: regenerable views of the log. Never authoritative; delete and rebuild freely.
//!
//! projections/graph.json   full time-indexed data (anchors, claims, commits, frames)
//! projections/view.html    the visual explorer, data inlined (opens from file://)
//! projections/wiki/        Karpathy-style pages: index.md, log.md, <kind>/<name>.md

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value as J};

use crate::frames::{self, Frame};
use crate::model::*;
use crate::relations;
use crate::state::State;
use crate::util::{now_rfc3339, path_parts};

const VIEWER: &str = include_str!("viewer.html");

pub struct BuildInfo<'a> {
    pub branch: &'a str,
    pub branches: &'a BTreeMap<String, Id>,
}

pub fn data_json(lineage: &[Commit], state: &State, frames: &[Frame], info: &BuildInfo) -> J {
    // Per-commit summary, including what each commit added and changed.
    let commits: Vec<J> = lineage
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (mut a, mut cl, mut s, mut st) = (0, 0, 0, 0);
            for op in &c.ops {
                match op {
                    Op::AddAnchor { .. } => a += 1,
                    Op::AddClaim { .. } => cl += 1,
                    Op::AddSource { .. } => s += 1,
                    Op::SetStatus { .. } => st += 1,
                }
            }
            json!({ "idx": i, "id": c.id, "author": c.author, "message": c.message, "time": c.time,
                    "added": { "anchors": a, "claims": cl, "sources": s, "status": st } })
        })
        .collect();

    let anchors: Vec<J> = state
        .anchors
        .values()
        .map(|a| {
            json!({ "id": a.anchor.id, "kind": a.anchor.kind, "name": a.anchor.name,
                    "scope": a.anchor.scope, "summary": a.anchor.summary, "aliases": a.anchor.aliases,
                    "introduced": a.introduced })
        })
        .collect();

    let claims: Vec<J> = state
        .claims
        .values()
        .map(|cs| {
            let c = &cs.claim;
            let (vk, vv) = match &c.value {
                Value::Anchor(a) => ("anchor", json!(a)),
                Value::Text(t) => ("text", json!(t)),
                Value::Number(n) => ("number", json!(n)),
                Value::Bool(b) => ("bool", json!(b)),
            };
            json!({ "id": c.id, "about": c.about, "relation": c.relation,
                    "family": relations::info(&c.relation).family,
                    "vk": vk, "v": vv, "scope": c.scope, "basis": c.basis.as_str(),
                    "stance": c.stance.as_str(), "confidence": c.confidence, "rationale": c.rationale,
                    "valid_from": c.valid_from, "author": c.author,
                    "evidence": c.evidence_sources(), "status": cs.status.as_str(),
                    "introduced": cs.introduced, "changed": cs.changed, "reinforced": cs.reinforced,
                    "replaced_by": cs.replaced_by })
        })
        .collect();

    let sources: Vec<J> = state
        .sources
        .values()
        .map(|s| json!({ "id": s.source.id, "kind": s.source.kind, "uri": s.source.uri,
                         "title": s.source.title, "captured_at": s.source.captured_at,
                         "introduced": s.introduced }))
        .collect();

    json!({
        "meta": { "built_at": now_rfc3339(), "branch": info.branch,
                  "head": lineage.last().map(|c| c.id.clone()), "branches": info.branches,
                  "families": relations::FAMILIES },
        "commits": commits, "frames": frames, "anchors": anchors, "claims": claims, "sources": sources
    })
}

pub fn write_all(out: &Path, lineage: &[Commit], state: &State, frames: &[Frame], info: &BuildInfo) -> Result<()> {
    fs::create_dir_all(out)?;
    let data = data_json(lineage, state, frames, info);
    let text = serde_json::to_string(&data)?;
    fs::write(out.join("graph.json"), &text)?;
    // Inline the data so the page works from file:// without a server. Escape `</` for <script>.
    let html = VIEWER.replace("/*MNEME_DATA*/null", &text.replace("</", "<\\/"));
    fs::write(out.join("view.html"), html)?;
    write_wiki(&out.join("wiki"), lineage, state, frames)?;
    Ok(())
}

fn page_link(from_kind_dir: bool, id: &str) -> String {
    let (k, n) = path_parts(id);
    if from_kind_dir {
        format!("../{k}/{n}.md")
    } else {
        format!("{k}/{n}.md")
    }
}

fn name_of(state: &State, id: &str) -> String {
    state.anchors.get(id).map(|a| a.anchor.name.clone()).unwrap_or_else(|| id.to_string())
}

fn render_claim(state: &State, cs: &crate::state::ClaimState, incoming: bool) -> String {
    let c = &cs.claim;
    let target = if incoming { Some(c.about.as_str()) } else { c.value.anchor() };
    let shown = match target {
        Some(t) => format!("[{}]({})", name_of(state, t), page_link(true, t)),
        None => c.value.display().replace('\n', " "),
    };
    let mut s = format!("{shown} — *{}*", c.basis.as_str());
    if c.stance != Stance::Is {
        let _ = write!(s, ", {}", c.stance.as_str());
    }
    if let Some(conf) = c.confidence {
        let _ = write!(s, ", conf {conf:.2}");
    }
    if let Some(v) = &c.valid_from {
        let _ = write!(s, ", since {}", &v[..v.len().min(10)]);
    }
    if cs.reinforced > 0 {
        let _ = write!(s, ", reinforced ×{}", cs.reinforced);
    }
    if let Some(r) = &c.rationale {
        let _ = write!(s, "<br>  ↳ {}", r.replace('\n', " "));
    }
    s
}

fn write_wiki(dir: &Path, lineage: &[Commit], state: &State, frames: &[Frame]) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;
    let frame_for = |kind: &str| frames.iter().find(|f| f.kind == kind);

    let mut by_kind: BTreeMap<String, Vec<(String, String, Option<f32>)>> = BTreeMap::new();

    for a in state.anchors.values() {
        let id = &a.anchor.id;
        let (k, n) = path_parts(id);
        fs::create_dir_all(dir.join(&k))?;
        let mut md = format!("# {}\n\n`{}` · kind **{}** · scope `{}` · known since commit {}\n\n",
            a.anchor.name, id, a.anchor.kind, a.anchor.scope, a.introduced);
        if let Some(s) = &a.anchor.summary {
            let _ = writeln!(md, "> {}\n", s.replace('\n', " "));
        }
        let mut cov = None;
        let mut used: Vec<&str> = vec![];
        if let Some(frame) = frame_for(&a.anchor.kind) {
            let fills = frames::profile(frame, state, id);
            let c = frames::coverage(&fills);
            cov = Some(c);
            let _ = writeln!(md, "## Understanding ({:.0}% of the {} frame)\n", c * 100.0, frame.label);
            let mut frontier = vec![];
            for f in &fills {
                if f.claims.is_empty() {
                    frontier.push(f.slot);
                    continue;
                }
                let _ = writeln!(md, "**{}**", f.slot.label);
                for cs in &f.claims {
                    used.push(&cs.claim.id);
                    let incoming = cs.claim.about != *id;
                    let _ = writeln!(md, "- {}", render_claim(state, cs, incoming));
                }
                md.push('\n');
            }
            if !frontier.is_empty() {
                let _ = writeln!(md, "## Frontier — not yet understood\n");
                for s in frontier {
                    let hint = s.hint.as_deref().map(|h| format!(" — {h}")).unwrap_or_default();
                    let _ = writeln!(md, "- [ ] **{}**{hint}", s.label);
                }
                md.push('\n');
            }
        }
        let other: Vec<_> = state.about(id).filter(|cs| !used.contains(&cs.claim.id.as_str())).collect();
        if !other.is_empty() {
            let _ = writeln!(md, "## Other claims\n");
            for cs in other {
                let _ = writeln!(md, "- `{}` {}", cs.claim.relation, render_claim(state, cs, false));
            }
            md.push('\n');
        }
        let incoming: Vec<_> = state.pointing_to(id).filter(|cs| !used.contains(&cs.claim.id.as_str())).collect();
        if !incoming.is_empty() {
            let _ = writeln!(md, "## Linked from\n");
            for cs in incoming {
                let _ = writeln!(md, "- `{}` ← {}", cs.claim.relation, render_claim(state, cs, true));
            }
            md.push('\n');
        }
        let mut srcs: Vec<&str> = state.about(id).flat_map(|cs| cs.claim.evidence_sources()).collect();
        srcs.sort();
        srcs.dedup();
        if !srcs.is_empty() {
            let _ = writeln!(md, "## Evidence\n");
            for s in srcs {
                if let Some(ss) = state.sources.get(s) {
                    let _ = writeln!(md, "- `{}` {} — {} ({})", s, ss.source.kind, ss.source.uri, &ss.source.captured_at[..10]);
                }
            }
        }
        fs::write(dir.join(&k).join(format!("{n}.md")), md)?;
        by_kind.entry(a.anchor.kind.clone()).or_default().push((a.anchor.name.clone(), id.clone(), cov));
    }

    let mut index = String::from("# mneme — index\n\nGenerated projection of the memory log. Do not edit; rebuild with `mneme build`.\n\n");
    for (kind, mut items) in by_kind {
        items.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        let _ = writeln!(index, "## {kind} ({})\n", items.len());
        for (name, id, cov) in items {
            let c = cov.map(|c| format!(" · {:.0}% understood", c * 100.0)).unwrap_or_default();
            let _ = writeln!(index, "- [{name}]({}){c}", page_link(false, &id));
        }
        index.push('\n');
    }
    fs::write(dir.join("index.md"), index)?;

    let mut log = String::from("# mneme — log\n\n");
    for (i, c) in lineage.iter().enumerate().rev() {
        let _ = writeln!(log, "- **{i}** `{}` {} — {} ({} ops) — *{}*", c.id, &c.time[..10], c.message, c.ops.len(), c.author);
    }
    fs::write(dir.join("log.md"), log)?;
    Ok(())
}
