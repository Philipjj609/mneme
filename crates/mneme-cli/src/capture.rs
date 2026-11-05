//! Capture adapters: turn outside things into Sources + observed/stated claims.

use anyhow::{Context, Result};
use mneme_core::util::{now_rfc3339, sha256_hex, short_hash};
use mneme_core::*;
use serde_json::Value as J;

const AUTHOR: &str = "mneme/capture-github@0.1";

fn get(url: &str) -> Result<(J, Option<String>)> {
    let mut req = ureq::get(url)
        .set("User-Agent", "mneme/0.1")
        .set("Accept", "application/vnd.github.star+json");
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let resp = req.call().with_context(|| format!("GET {url}"))?;
    let link = resp.header("link").map(String::from);
    Ok((serde_json::from_str(&resp.into_string()?)?, link))
}

pub fn github_stars(repo: &Repo, state: &State, user: &str, person: &str) -> Result<Vec<Op>> {
    let mut sim = state.clone();
    let mut ops: Vec<Op> = vec![];
    let mut push = |sim: &mut State, op: Op| {
        sim.apply(&op, usize::MAX);
        ops.push(op);
    };
    let now = now_rfc3339();
    let today = now[..10].to_string();

    if !sim.anchors.contains_key(person) {
        push(&mut sim, Op::AddAnchor { anchor: Anchor {
            id: person.into(), kind: "person".into(), name: "Philip".into(), aliases: vec![user.into()],
            scope: "personal:philip".into(), summary: None } });
    }

    let mut page = 1;
    loop {
        let url = format!("https://api.github.com/users/{user}/starred?per_page=100&page={page}");
        let (body, link) = get(&url)?;
        let items = body.as_array().cloned().unwrap_or_default();
        eprintln!("page {page}: {} stars", items.len());
        for item in &items {
            let r = &item["repo"];
            let full = r["full_name"].as_str().unwrap_or_default().to_string();
            if full.is_empty() {
                continue;
            }
            // The source is exactly what GitHub returned for this star.
            let payload = serde_json::to_string_pretty(item)?;
            let sid = short_hash("s_", payload.as_bytes());
            let aid = format!("repo:{full}");
            let desc = r["description"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            push(&mut sim, Op::AddAnchor { anchor: Anchor {
                id: aid.clone(), kind: "repo".into(), name: full.clone(), aliases: vec![],
                scope: "world".into(), summary: desc.clone() } });

            let mut claims: Vec<Claim> = vec![];
            if let Some(d) = desc {
                claims.push(Claim::new(&aid, "described_as", Value::Text(d), Basis::Stated, AUTHOR));
            }
            if let Some(l) = r["language"].as_str() {
                claims.push(Claim::new(&aid, "language", Value::Text(l.into()), Basis::Observed, AUTHOR));
            }
            if let Some(n) = r["stargazers_count"].as_f64() {
                let mut c = Claim::new(&aid, "stars", Value::Number(n), Basis::Observed, AUTHOR);
                c.valid_from = Some(today.clone());
                claims.push(c);
            }
            if let Some(l) = r["license"]["spdx_id"].as_str().filter(|l| *l != "NOASSERTION") {
                claims.push(Claim::new(&aid, "license", Value::Text(l.into()), Basis::Observed, AUTHOR));
            }
            if let Some(h) = r["homepage"].as_str().filter(|h| !h.is_empty()) {
                claims.push(Claim::new(&aid, "homepage", Value::Text(h.into()), Basis::Observed, AUTHOR));
            }
            if let Some(owner) = r["owner"]["login"].as_str() {
                let oid = format!("owner:{owner}");
                if !sim.anchors.contains_key(&oid) {
                    push(&mut sim, Op::AddAnchor { anchor: Anchor {
                        id: oid.clone(), kind: "owner".into(), name: owner.into(), aliases: vec![],
                        scope: "world".into(), summary: None } });
                }
                claims.push(Claim::new(&aid, "owned_by", Value::Anchor(oid), Basis::Observed, AUTHOR));
            }
            for t in r["topics"].as_array().into_iter().flatten().filter_map(|t| t.as_str()) {
                let tid = format!("topic:{t}");
                if !sim.anchors.contains_key(&tid) {
                    push(&mut sim, Op::AddAnchor { anchor: Anchor {
                        id: tid.clone(), kind: "topic".into(), name: t.into(), aliases: vec![],
                        scope: "world".into(), summary: None } });
                }
                claims.push(Claim::new(&aid, "tagged", Value::Anchor(tid), Basis::Observed, AUTHOR));
            }
            let mut starred = Claim::new(&aid, "starred_by", Value::Anchor(person.into()), Basis::Observed, AUTHOR);
            starred.valid_from = item["starred_at"].as_str().map(String::from);
            starred.scope = "personal:philip".into();
            claims.push(starred);

            // Record only what changed: a fact already known and active is not re-recorded just
            // because GitHub was asked again. The new snapshot is kept as a source only if it
            // taught us something (e.g. a new star count, which supersedes the old one).
            let mut changes = vec![];
            for c in claims {
                let c = c.with_evidence(&sid).finalize();
                if sim.claims.get(&c.id).is_some_and(|k| k.status == Status::Active) {
                    continue;
                }
                changes.extend(sim.assert_ops(c));
            }
            if !changes.is_empty() && !sim.sources.contains_key(&sid) {
                repo.save_source_payload(&sid, "json", payload.as_bytes())?;
                push(&mut sim, Op::AddSource { source: Source {
                    id: sid.clone(), kind: "github-star".into(), uri: r["html_url"].as_str().unwrap_or_default().into(),
                    title: full.clone(), captured_at: now.clone(), content_hash: sha256_hex(payload.as_bytes()),
                    meta: Default::default() } });
            }
            for op in changes {
                push(&mut sim, op);
            }
        }
        let more = link.as_deref().is_some_and(|l| l.contains("rel=\"next\""));
        if !more || items.is_empty() {
            break;
        }
        page += 1;
    }
    // Drop anchor re-assertions that change nothing, so a re-capture only records real change.
    let ops = ops
        .into_iter()
        .filter(|op| match op {
            Op::AddAnchor { anchor } => match state.anchors.get(&anchor.id) {
                Some(old) => old.anchor.summary != anchor.summary || old.anchor.name != anchor.name,
                None => true,
            },
            _ => true,
        })
        .collect();
    Ok(ops)
}
