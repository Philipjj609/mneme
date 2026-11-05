//! Digest proposals: the write path for interpretation.
//!
//! A model (or a person) writes a proposal file. mneme validates every reference, resolves values,
//! attaches evidence, archives the proposal itself as a source, and turns it into ops.
//!
//! ```json
//! { "author": "claude-opus-5-5", "message": "…", "basis": "inferred", "confidence": 0.7,
//!   "evidence": "auto",
//!   "anchors": [ { "id": "concept:agent-memory", "name": "Agent memory", "summary": "…" } ],
//!   "claims":  [ ["repo:a/b", "provides", "concept:agent-memory"],
//!                ["repo:a/b", "relevant_to", "project:mneme", 0.8, "why"],
//!                { "about": "…", "relation": "…", "value": "…", "basis": "stated", "stance": "could" } ] }
//! ```
//! A string value that names a known anchor becomes a link; anything else is text.

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::Value as J;

use crate::model::*;
use crate::state::State;
use crate::util::kind_of;

#[derive(Debug, Deserialize)]
pub struct Proposal {
    pub author: String,
    pub message: String,
    #[serde(default = "default_basis")]
    pub basis: Basis,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default = "default_scope")]
    pub scope: String,
    /// "auto": cite the sources behind observed/stated claims about the subject (and linked anchor).
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    #[serde(default)]
    pub claims: Vec<J>,
}

fn default_basis() -> Basis {
    Basis::Inferred
}

pub fn ops_from_proposal(state: &State, p: &Proposal, proposal_source: Source) -> Result<Vec<Op>> {
    let mut sim = state.clone();
    let mut ops = vec![];
    let src_id = proposal_source.id.clone();
    let add = |sim: &mut State, ops: &mut Vec<Op>, op: Op| {
        sim.apply(&op, usize::MAX);
        ops.push(op);
    };

    add(&mut sim, &mut ops, Op::AddSource { source: proposal_source });

    for a in &p.anchors {
        let mut a = a.clone();
        if !a.id.contains(':') {
            bail!("anchor id '{}' must look like kind:name", a.id);
        }
        if a.kind.is_empty() {
            a.kind = kind_of(&a.id).to_string();
        }
        add(&mut sim, &mut ops, Op::AddAnchor { anchor: a });
    }

    let mut errors = vec![];
    for (i, raw) in p.claims.iter().enumerate() {
        match parse_claim(&sim, p, raw) {
            Ok(mut claim) => {
                if p.evidence.as_deref() == Some("auto") && claim.evidence.is_empty() {
                    let mut srcs = sim.sources_supporting(&claim.about);
                    if let Some(v) = claim.value.anchor() {
                        for s in sim.sources_supporting(v) {
                            if !srcs.contains(&s) {
                                srcs.push(s);
                            }
                        }
                    }
                    for s in srcs {
                        claim = claim.with_evidence(&s);
                    }
                }
                if claim.evidence.is_empty() {
                    // No external evidence: the archived proposal is the record of the reasoning.
                    claim = claim.with_evidence(&src_id);
                }
                let claim = claim.finalize();
                for op in sim.assert_ops(claim) {
                    add(&mut sim, &mut ops, op);
                }
            }
            Err(e) => errors.push(format!("claim #{i}: {e}")),
        }
    }
    if !errors.is_empty() {
        bail!("proposal rejected:\n  {}", errors.join("\n  "));
    }
    Ok(ops)
}

fn looks_like_id(s: &str) -> bool {
    const KINDS: &[&str] = &[
        "concept", "repo", "site", "post", "project", "stage", "account", "dataset", "topic", "owner", "person",
    ];
    match s.split_once(':') {
        Some((k, rest)) => KINDS.contains(&k) && !rest.is_empty() && !rest.contains(char::is_whitespace),
        None => false,
    }
}

fn resolve_value(sim: &State, v: &J) -> Result<Value> {
    Ok(match v {
        J::String(s) if sim.anchors.contains_key(s) => Value::Anchor(s.clone()),
        // Looks like an id but names nothing known: almost always a typo or a missing anchor.
        J::String(s) if looks_like_id(s) => bail!("value '{s}' looks like an anchor id but no such anchor exists"),
        J::String(s) => Value::Text(s.clone()),
        J::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        J::Bool(b) => Value::Bool(*b),
        J::Object(_) => serde_json::from_value(v.clone())?,
        _ => bail!("unsupported value {v}"),
    })
}

fn parse_claim(sim: &State, p: &Proposal, raw: &J) -> Result<Claim> {
    let s = |v: Option<&J>, what: &str| -> Result<String> {
        v.and_then(|x| x.as_str()).map(String::from).ok_or_else(|| anyhow!("missing {what}"))
    };
    let mut claim = match raw {
        J::Array(a) => {
            let about = s(a.first(), "about")?;
            let rel = s(a.get(1), "relation")?;
            let value = resolve_value(sim, a.get(2).ok_or_else(|| anyhow!("missing value"))?)?;
            let mut c = Claim::new(&about, &rel, value, p.basis, &p.author);
            c.confidence = a.get(3).and_then(|x| x.as_f64()).map(|x| x as f32).or(p.confidence);
            c.rationale = a.get(4).and_then(|x| x.as_str()).map(String::from);
            c
        }
        J::Object(o) => {
            let about = s(o.get("about"), "about")?;
            let rel = s(o.get("relation"), "relation")?;
            let value = resolve_value(sim, o.get("value").ok_or_else(|| anyhow!("missing value"))?)?;
            let basis = match o.get("basis") {
                Some(b) => serde_json::from_value(b.clone())?,
                None => p.basis,
            };
            let mut c = Claim::new(&about, &rel, value, basis, &p.author);
            if let Some(st) = o.get("stance") {
                c.stance = serde_json::from_value(st.clone())?;
            }
            c.confidence = o.get("confidence").and_then(|x| x.as_f64()).map(|x| x as f32).or(p.confidence);
            c.rationale = o.get("rationale").and_then(|x| x.as_str()).map(String::from);
            if let Some(sc) = o.get("scope").and_then(|x| x.as_str()) {
                c.scope = sc.into();
            }
            c.valid_from = o.get("valid_from").and_then(|x| x.as_str()).map(String::from);
            // Explicit evidence: source ids, each an independent group.
            for s in o.get("evidence").and_then(|x| x.as_array()).into_iter().flatten().filter_map(|x| x.as_str()) {
                if !sim.sources.contains_key(s) {
                    bail!("unknown evidence source '{s}'");
                }
                c = c.with_evidence(s);
            }
            c
        }
        _ => bail!("claim must be an array or object"),
    };
    if claim.scope == default_scope() {
        claim.scope = p.scope.clone();
    }
    if !sim.exists(&claim.about) {
        bail!("unknown subject '{}'", claim.about);
    }
    if claim.basis != Basis::Inferred {
        // Only inferences carry a confidence; observed/stated facts are what they are.
        claim.confidence = None;
    }
    Ok(claim)
}
