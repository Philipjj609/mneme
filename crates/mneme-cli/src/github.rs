//! GitHub capture: a user's stars and repos' READMEs as sources + observed claims.
//! Only change is recorded: re-observing a known fact is not a new commit; a changed star count
//! supersedes the old one.

use mneme_core::model::{Anchor, AnchorId, Basis, Claim, Op, Scope, Source, SourceId, Value};
use mneme_core::util::now_rfc3339;
use mneme_core::{Batch, Repo, State};
use organon_protocol::sha256_hex;
use serde_json::Value as J;

use crate::net::{github_page, Fetch, Http};

const AUTHOR: &str = "mneme/capture-github@0.2";

fn source(id: SourceId, kind: &str, uri: &str, title: &str, now: &str, content: &str) -> Source {
    Source {
        id,
        kind: kind.to_owned(),
        uri: uri.to_owned(),
        title: title.to_owned(),
        captured_at: now.to_owned(),
        content_hash: sha256_hex(content.as_bytes()),
        files: Default::default(),
        meta: Default::default(),
        produced_by: None,
    }
}

fn anchor(id: &str, name: &str, scope: &str, summary: Option<String>) -> anyhow::Result<Anchor> {
    Ok(Anchor {
        id: id.parse()?,
        name: name.to_owned(),
        aliases: Vec::new(),
        scope: Scope::new(scope),
        summary,
    })
}

/// The observed claims one starred-repo record supports.
fn star_claims(
    item: &J,
    repo_id: &AnchorId,
    person: &AnchorId,
    today: &str,
    batch: &mut Batch,
) -> anyhow::Result<Vec<Claim>> {
    let r = &item["repo"];
    let mut claims = Vec::new();
    let add = |relation: &str, value: Value, basis: Basis| {
        Claim::draft(repo_id.clone(), relation, value, basis, AUTHOR)
    };
    if let Some(d) = r["description"]
        .as_str()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        claims.push(add(
            "described_as",
            Value::Text(d.to_owned()),
            Basis::Stated,
        ));
    }
    if let Some(l) = r["language"].as_str() {
        claims.push(add("language", Value::Text(l.to_owned()), Basis::Observed));
    }
    if let Some(n) = r["stargazers_count"].as_f64() {
        let mut c = add("stars", Value::Number(n), Basis::Observed);
        c.valid_from = Some(today.to_owned());
        claims.push(c);
    }
    if let Some(l) = r["license"]["spdx_id"]
        .as_str()
        .filter(|l| *l != "NOASSERTION")
    {
        claims.push(add("license", Value::Text(l.to_owned()), Basis::Observed));
    }
    if let Some(h) = r["homepage"].as_str().filter(|h| !h.is_empty()) {
        claims.push(add("homepage", Value::Text(h.to_owned()), Basis::Observed));
    }
    if let Some(owner) = r["owner"]["login"].as_str() {
        let id = format!("owner:{owner}");
        if batch.anchor(&id).is_none() {
            batch.push(Op::AddAnchor {
                anchor: anchor(&id, owner, "world", None)?,
            });
        }
        claims.push(add("owned_by", Value::Anchor(id.parse()?), Basis::Observed));
    }
    for topic in r["topics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(J::as_str)
    {
        let id = format!("topic:{topic}");
        if batch.anchor(&id).is_none() {
            batch.push(Op::AddAnchor {
                anchor: anchor(&id, topic, "world", None)?,
            });
        }
        claims.push(add("tagged", Value::Anchor(id.parse()?), Basis::Observed));
    }
    let mut starred = add("starred_by", Value::Anchor(person.clone()), Basis::Observed);
    starred.valid_from = item["starred_at"].as_str().map(str::to_owned);
    starred.scope = Scope::new("personal:philip");
    claims.push(starred);
    Ok(claims
        .into_iter()
        .map(mneme_core::model::ClaimBody::finalize)
        .collect())
}

/// Ops for `user`'s starred repositories.
///
/// # Errors
/// Network or storage failure.
pub fn stars(
    repo: &Repo,
    state: &State,
    http: &Http,
    user: &str,
    person: &str,
    log: &mut dyn FnMut(&str),
) -> anyhow::Result<Vec<Op>> {
    let mut batch = Batch::new(state);
    let now = now_rfc3339();
    let today = now[..10].to_owned();
    let person_id: AnchorId = person.parse()?;
    if batch.anchor(person).is_none() {
        let mut p = anchor(person, "Philip", "personal:philip", None)?;
        p.aliases.push(user.to_owned());
        batch.push(Op::AddAnchor { anchor: p });
    }
    let mut page = 1;
    loop {
        let (body, has_next) = github_page(
            http,
            &format!("https://api.github.com/users/{user}/starred?per_page=100&page={page}"),
        )?;
        let items = body.as_array().cloned().unwrap_or_default();
        log(&format!("page {page}: {} stars", items.len()));
        for item in &items {
            let Some(full) = item["repo"]["full_name"].as_str() else {
                continue;
            };
            let repo_id: AnchorId = format!("repo:{full}").parse()?;
            let summary = item["repo"]["description"]
                .as_str()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            batch.upsert_anchor(anchor(repo_id.as_str(), full, "world", summary)?);
            let payload = serde_json::to_string_pretty(item)?;
            let sid = SourceId::of(payload.as_bytes());
            let fresh: Vec<Claim> = star_claims(item, &repo_id, &person_id, &today, &mut batch)?
                .into_iter()
                .map(|c| Claim {
                    body: c.body.with_evidence(sid.clone()),
                    ..c
                })
                .filter(|c| batch.claim_status(&c.id) != Some(mneme_core::model::Status::Active))
                .collect();
            // Keep the snapshot only if it taught us something (e.g. a new star count).
            if !fresh.is_empty() && !batch.has_source(&sid) {
                repo.save_source_payload(sid.as_str(), "json", payload.as_bytes())?;
                let uri = item["repo"]["html_url"].as_str().unwrap_or_default();
                batch.add_source(source(
                    sid.clone(),
                    "github-star",
                    uri,
                    full,
                    &now,
                    &payload,
                ));
            }
            for claim in fresh {
                batch.assert(claim);
            }
        }
        if !has_next || items.is_empty() {
            break;
        }
        page += 1;
    }
    Ok(batch.into_ops())
}

/// Ops adding the README of each repo (already in memory) as a source, cited by a
/// `documented_in` claim.
///
/// # Errors
/// A repo not in memory, or a network/storage failure.
pub fn readmes(
    repo: &Repo,
    state: &State,
    fetch: &dyn Fetch,
    names: &[String],
    log: &mut dyn FnMut(&str),
) -> anyhow::Result<Vec<Op>> {
    let mut batch = Batch::new(state);
    let now = now_rfc3339();
    for full in names {
        let repo_id: AnchorId = format!("repo:{full}").parse()?;
        anyhow::ensure!(
            batch.anchor(repo_id.as_str()).is_some(),
            "{repo_id} is not in memory yet (capture it first)"
        );
        let text = fetch.text(
            &format!("https://api.github.com/repos/{full}/readme"),
            "application/vnd.github.raw",
        )?;
        let sid = SourceId::of(text.as_bytes());
        if batch.has_source(&sid) {
            log(&format!("{full}: README unchanged"));
            continue;
        }
        repo.save_source_payload(sid.as_str(), "md", text.as_bytes())?;
        log(&format!("{full}: README {} chars -> {sid}", text.len()));
        batch.add_source(source(
            sid.clone(),
            "github-readme",
            &format!("https://github.com/{full}#readme"),
            &format!("{full} README"),
            &now,
            &text,
        ));
        let claim = Claim::draft(
            repo_id,
            "documented_in",
            Value::Text(format!("README ({} chars)", text.len())),
            Basis::Observed,
            AUTHOR,
        )
        .with_evidence(sid)
        .finalize();
        batch.assert(claim);
    }
    Ok(batch.into_ops())
}
