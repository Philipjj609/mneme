//! Digest: turn a protocol [`Proposal`] into ops. Validates every subject, resolves string
//! values to links when they name a known anchor (and rejects id-like strings that name
//! nothing), attaches evidence, and archives the proposal itself as a source.

use organon_protocol::{AnchorId, Proposal, ProposedClaim};
use serde_json::{Map, Value as J};

use crate::error::{Error, Result};
use crate::model::{
    Basis, Claim, ClaimBody, EvidenceRef, Op, Scope, Source, SourceId, Stance, Subject, Value,
};
use crate::state::{Batch, State};

/// Ops for `proposal` against `state`. `archive` is the source recording the proposal text.
///
/// # Errors
/// [`Error::Rejected`] listing every invalid claim (nothing is committed then).
pub fn ops_from_proposal(state: &State, proposal: &Proposal, archive: Source) -> Result<Vec<Op>> {
    let mut batch = Batch::new(state);
    let archive_id = archive.id.clone();
    batch.add_source(archive);
    for anchor in &proposal.anchors {
        batch.push(Op::AddAnchor {
            anchor: anchor.clone(),
        });
    }

    let mut errors = Vec::new();
    for (i, raw) in proposal.claims.iter().enumerate() {
        match parse_claim(&batch, proposal, raw) {
            Ok(mut body) => {
                if proposal.evidence.is_some() && body.evidence.is_empty() {
                    attach_auto_evidence(&batch, &mut body);
                }
                if body.evidence.is_empty() {
                    // No outside evidence: the archived proposal is the record of the reasoning.
                    body = body.with_evidence(archive_id.clone());
                }
                batch.assert(body.finalize());
            }
            Err(e) => errors.push(format!("claim #{i}: {e}")),
        }
    }
    if errors.is_empty() {
        Ok(batch.into_ops())
    } else {
        Err(Error::Rejected(errors))
    }
}

fn attach_auto_evidence(batch: &Batch, body: &mut ClaimBody) {
    let mut sources = batch.sources_supporting(body.about.as_str());
    if let Some(target) = body.value.anchor() {
        for s in batch.sources_supporting(target.as_str()) {
            if !sources.contains(&s) {
                sources.push(s);
            }
        }
    }
    body.evidence
        .extend(sources.into_iter().map(|s| vec![EvidenceRef::source(s)]));
}

/// A string naming a known anchor becomes a link; an id-like string naming nothing is an error.
fn resolve_value(batch: &Batch, v: &J) -> std::result::Result<Value, String> {
    match v {
        J::String(s) if batch.anchor(s).is_some() => s
            .parse::<AnchorId>()
            .map(Value::Anchor)
            .map_err(|e| e.to_string()),
        J::String(s) if looks_like_known_kind(s) => Err(format!(
            "value '{s}' looks like an anchor id but no such anchor exists"
        )),
        J::String(s) => Ok(Value::Text(s.clone())),
        J::Number(n) => n
            .as_f64()
            .map(Value::Number)
            .ok_or_else(|| format!("number {n} out of range")),
        J::Bool(b) => Ok(Value::Bool(*b)),
        J::Object(_) => serde_json::from_value(v.clone()).map_err(|e| e.to_string()),
        _ => Err(format!("unsupported value {v}")),
    }
}

/// Kinds whose ids are worth guarding against typos (free text like "note: x" must stay text).
fn looks_like_known_kind(s: &str) -> bool {
    const KINDS: &[&str] = &[
        "concept", "repo", "site", "post", "project", "stage", "account", "dataset", "topic",
        "owner", "person",
    ];
    AnchorId::looks_like(s) && s.split_once(':').is_some_and(|(k, _)| KINDS.contains(&k))
}

#[allow(clippy::cast_possible_truncation)]
fn as_f32(v: Option<&J>) -> Option<f32> {
    v.and_then(J::as_f64).map(|x| x as f32)
}

fn as_string(v: Option<&J>, what: &str) -> std::result::Result<String, String> {
    v.and_then(J::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing {what}"))
}

fn parse_claim(
    batch: &Batch,
    p: &Proposal,
    raw: &ProposedClaim,
) -> std::result::Result<ClaimBody, String> {
    let mut body = match raw {
        ProposedClaim::Short(items) => {
            let about = as_string(items.first(), "about")?;
            let relation = as_string(items.get(1), "relation")?;
            let value = resolve_value(batch, items.get(2).ok_or("missing value")?)?;
            let mut body = Claim::draft(
                subject(&about)?,
                relation.as_str(),
                value,
                p.basis,
                &p.author,
            );
            body.confidence = as_f32(items.get(3)).or(p.confidence);
            body.rationale = items.get(4).and_then(J::as_str).map(str::to_owned);
            body
        }
        ProposedClaim::Full(o) => parse_full(batch, p, o)?,
    };
    if body.scope.is_world() && !p.scope.is_world() {
        body.scope = p.scope.clone();
    }
    if !batch.knows_subject(body.about.as_str()) {
        return Err(format!("unknown subject '{}'", body.about));
    }
    if body.basis != Basis::Inferred {
        // Only inferences carry a confidence; observed and stated facts are what they are.
        body.confidence = None;
    }
    Ok(body)
}

fn parse_full(
    batch: &Batch,
    p: &Proposal,
    o: &Map<String, J>,
) -> std::result::Result<ClaimBody, String> {
    let about = as_string(o.get("about"), "about")?;
    let relation = as_string(o.get("relation"), "relation")?;
    let value = resolve_value(batch, o.get("value").ok_or("missing value")?)?;
    let basis: Basis = match o.get("basis") {
        Some(b) => serde_json::from_value(b.clone()).map_err(|e| format!("basis: {e}"))?,
        None => p.basis,
    };
    let mut body = Claim::draft(subject(&about)?, relation.as_str(), value, basis, &p.author);
    if let Some(st) = o.get("stance") {
        body.stance =
            serde_json::from_value::<Stance>(st.clone()).map_err(|e| format!("stance: {e}"))?;
    }
    body.confidence = as_f32(o.get("confidence")).or(p.confidence);
    body.rationale = o.get("rationale").and_then(J::as_str).map(str::to_owned);
    if let Some(sc) = o.get("scope").and_then(J::as_str) {
        body.scope = Scope::new(sc);
    }
    body.valid_from = o.get("valid_from").and_then(J::as_str).map(str::to_owned);
    for s in o
        .get("evidence")
        .and_then(J::as_array)
        .into_iter()
        .flatten()
        .filter_map(J::as_str)
    {
        let id: SourceId = s.parse().map_err(|e| format!("evidence: {e}"))?;
        if !batch.has_source(&id) {
            return Err(format!("unknown evidence source '{s}'"));
        }
        body = body.with_evidence(id);
    }
    Ok(body)
}

fn subject(s: &str) -> std::result::Result<Subject, String> {
    s.parse::<Subject>().map_err(|e| e.to_string())
}
