//! Ingest: what an organ perceived (organon-protocol messages) into the memory.
//!
//! Sources, anchors and observed facts become ops; observations go to the side store. This is
//! capture, not interpretation, so the rules are strict:
//! - a claim must be `observed` or `imported` (inferences enter only through `digest`),
//! - its id must match its content (ids are contracts),
//! - it must name a known subject and carry evidence that resolves to a known source or
//!   observation (here or already in the memory),
//! - an observation's id must match its content and its source must be known.
//!
//! Every problem is reported at once; nothing is committed if any message is rejected.

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::model::{Basis, Claim, EvidenceRef, Message, Observation, ObservationId, Op};
use crate::state::{Batch, State};

/// What a message stream turns into.
#[derive(Debug, Default)]
pub struct Ingested {
    /// Ops for one commit (may be empty when everything was already known).
    pub ops: Vec<Op>,
    /// Observations for the side store (the store skips ones it already has).
    pub observations: Vec<Observation>,
}

/// Turn `messages` into ops and observations against `state`. `known` holds the ids of
/// observations already stored.
///
/// # Errors
/// [`Error::Rejected`] listing every invalid message, by line number (1-based).
pub fn ops_from_messages(
    state: &State,
    messages: Vec<Message>,
    known: &HashSet<ObservationId>,
) -> Result<Ingested> {
    let mut batch = Batch::new(state);
    let mut errors = Vec::new();
    let mut observations: Vec<Observation> = Vec::new();
    let mut claims: Vec<(usize, Claim)> = Vec::new();

    // Sources and anchors first, so claims and observations may come in any order.
    for (i, m) in messages.into_iter().enumerate() {
        let line = i + 1;
        match m {
            Message::Source(s) => batch.add_source(s),
            // A placeholder (name = the id's name part, no summary) only creates an anchor;
            // it never renames one memory already knows (organon-text emits these for links).
            Message::Anchor(a) if is_placeholder(&a) && batch.anchor(a.id.as_str()).is_some() => {}
            Message::Anchor(a) => batch.upsert_anchor(a),
            Message::Observation(o) => observations.push(o),
            Message::Claim(c) => claims.push((line, c)),
            Message::Proposal(_) => errors.push(format!(
                "line {line}: a proposal is interpretation; run `mneme digest` on it"
            )),
            Message::Episode(_) => errors.push(format!(
                "line {line}: episodes are usage, not knowledge; not ingested"
            )),
        }
    }

    for o in &observations {
        let expected = Observation::compute_id(&o.source, &o.kind, &o.at, &o.value);
        if o.id != expected {
            errors.push(format!(
                "observation {}: id does not match content (expected {expected})",
                o.id
            ));
        }
        if !batch.has_source(&o.source) {
            errors.push(format!("observation {}: unknown source {}", o.id, o.source));
        }
    }

    let citable: HashSet<&ObservationId> = known
        .iter()
        .chain(observations.iter().map(|o| &o.id))
        .collect();
    for (line, claim) in claims {
        match check_claim(&batch, &citable, &claim) {
            Ok(()) => batch.assert_if_new(claim),
            Err(e) => errors.push(format!("line {line}: claim {}: {e}", claim.id)),
        }
    }

    if errors.is_empty() {
        Ok(Ingested {
            ops: batch.into_ops(),
            observations,
        })
    } else {
        Err(Error::Rejected(errors))
    }
}

/// An anchor that says nothing but its id: `site:a.dev` named `a.dev`, without a summary.
fn is_placeholder(a: &organon_protocol::Anchor) -> bool {
    a.summary.is_none() && a.name == a.id.name()
}

fn check_claim(
    batch: &Batch,
    citable: &HashSet<&ObservationId>,
    claim: &Claim,
) -> std::result::Result<(), String> {
    if !matches!(claim.basis, Basis::Observed | Basis::Imported) {
        return Err(format!(
            "basis '{:?}' is interpretation; send it as a proposal through `mneme digest`",
            claim.basis
        ));
    }
    let expected = claim.body.compute_id();
    if claim.id != expected {
        return Err(format!("id does not match content (expected {expected})"));
    }
    if !batch.knows_subject(claim.about.as_str()) {
        return Err(format!("unknown subject '{}'", claim.about));
    }
    if claim.evidence.is_empty() {
        return Err("no evidence".into());
    }
    for r in claim.evidence.iter().flatten() {
        match r {
            EvidenceRef::Source { source, .. } if !batch.has_source(source) => {
                return Err(format!("unknown evidence source {source}"));
            }
            EvidenceRef::Observation { observation } if !citable.contains(observation) => {
                return Err(format!("unknown evidence observation {observation}"));
            }
            _ => {}
        }
    }
    Ok(())
}
