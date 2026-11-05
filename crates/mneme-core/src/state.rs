//! Materialized state: what the memory holds at a given commit, obtained by replaying the log.

use std::collections::BTreeMap;

use crate::model::*;
use crate::relations;

#[derive(Clone, Debug)]
pub struct SourceState {
    pub source: Source,
    pub introduced: usize,
}

#[derive(Clone, Debug)]
pub struct AnchorState {
    pub anchor: Anchor,
    pub introduced: usize,
}

#[derive(Clone, Debug)]
pub struct ClaimState {
    pub claim: Claim,
    pub status: Status,
    /// Index (in the lineage) of the commit that first asserted it.
    pub introduced: usize,
    /// Index of the commit that last changed its status, if any.
    pub changed: Option<usize>,
    /// How many times it was asserted again with new evidence.
    pub reinforced: u32,
    pub replaced_by: Option<Id>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub sources: BTreeMap<Id, SourceState>,
    pub anchors: BTreeMap<Id, AnchorState>,
    pub claims: BTreeMap<Id, ClaimState>,
}

impl State {
    pub fn replay(lineage: &[Commit]) -> State {
        let mut s = State::default();
        for (i, c) in lineage.iter().enumerate() {
            for op in &c.ops {
                s.apply(op, i);
            }
        }
        s
    }

    pub fn apply(&mut self, op: &Op, idx: usize) {
        match op {
            Op::AddSource { source } => {
                self.sources
                    .entry(source.id.clone())
                    .or_insert(SourceState { source: source.clone(), introduced: idx });
            }
            Op::AddAnchor { anchor } => match self.anchors.get_mut(&anchor.id) {
                Some(existing) => {
                    for a in &anchor.aliases {
                        if !existing.anchor.aliases.contains(a) {
                            existing.anchor.aliases.push(a.clone());
                        }
                    }
                    if anchor.summary.is_some() {
                        existing.anchor.summary = anchor.summary.clone();
                    }
                    if !anchor.name.is_empty() {
                        existing.anchor.name = anchor.name.clone();
                    }
                }
                None => {
                    self.anchors
                        .insert(anchor.id.clone(), AnchorState { anchor: anchor.clone(), introduced: idx });
                }
            },
            Op::AddClaim { claim } => match self.claims.get_mut(&claim.id) {
                Some(existing) => {
                    let mut added = false;
                    for g in &claim.evidence {
                        if !existing.claim.evidence.contains(g) {
                            existing.claim.evidence.push(g.clone());
                            added = true;
                        }
                    }
                    if added {
                        existing.reinforced += 1;
                    }
                }
                None => {
                    self.claims.insert(
                        claim.id.clone(),
                        ClaimState {
                            claim: claim.clone(),
                            status: Status::Active,
                            introduced: idx,
                            changed: None,
                            reinforced: 0,
                            replaced_by: None,
                            reason: None,
                        },
                    );
                }
            },
            Op::SetStatus { claim, status, reason, replaced_by } => {
                if let Some(cs) = self.claims.get_mut(claim) {
                    cs.status = *status;
                    cs.changed = Some(idx);
                    cs.reason = reason.clone();
                    cs.replaced_by = replaced_by.clone();
                }
            }
        }
    }

    pub fn active(&self) -> impl Iterator<Item = &ClaimState> {
        self.claims.values().filter(|c| c.status == Status::Active)
    }

    pub fn about<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a ClaimState> + 'a {
        self.active().filter(move |c| c.claim.about == id)
    }

    pub fn pointing_to<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a ClaimState> + 'a {
        self.active().filter(move |c| c.claim.value.anchor() == Some(id))
    }

    pub fn exists(&self, id: &str) -> bool {
        self.anchors.contains_key(id) || self.claims.contains_key(id)
    }

    /// Sources behind observed/stated claims about an anchor: the evidence an inference can rest on.
    pub fn sources_supporting(&self, anchor: &str) -> Vec<Id> {
        let mut out: Vec<Id> = vec![];
        for c in self.about(anchor) {
            if matches!(c.claim.basis, Basis::Observed | Basis::Stated) {
                for s in c.claim.evidence_sources() {
                    if !out.iter().any(|o| o == s) {
                        out.push(s.to_string());
                    }
                }
            }
        }
        out
    }

    /// Ops needed to assert `claim` against the current state:
    /// nothing if already known with the same evidence; a reinforcement if the evidence is new;
    /// for single-valued relations, supersede the previous value rather than overwrite it.
    pub fn assert_ops(&self, claim: Claim) -> Vec<Op> {
        if let Some(existing) = self.claims.get(&claim.id) {
            let new_evidence = claim.evidence.iter().any(|g| !existing.claim.evidence.contains(g));
            return if new_evidence && existing.status == Status::Active {
                vec![Op::AddClaim { claim }]
            } else {
                vec![]
            };
        }
        let mut ops = vec![];
        if relations::info(&claim.relation).functional {
            for cs in self.active() {
                let c = &cs.claim;
                if c.about == claim.about && c.relation == claim.relation && c.scope == claim.scope {
                    ops.push(Op::SetStatus {
                        claim: c.id.clone(),
                        status: Status::Superseded,
                        reason: Some("newer value".into()),
                        replaced_by: Some(claim.id.clone()),
                    });
                }
            }
        }
        ops.push(Op::AddClaim { claim });
        ops
    }
}
