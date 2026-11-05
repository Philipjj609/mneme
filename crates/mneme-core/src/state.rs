//! What the memory holds at a commit, obtained by replaying the log, plus [`Batch`]: changes
//! staged on top of a state without copying it.

use std::collections::{BTreeMap, HashMap};

use crate::model::{
    Anchor, AnchorId, Basis, Claim, ClaimId, Commit, EvidenceRef, Op, Source, SourceId, Status,
};

#[derive(Debug, Clone)]
pub struct SourceState {
    pub source: Source,
    /// Index in the lineage of the commit that added it.
    pub introduced: usize,
}

#[derive(Debug, Clone)]
pub struct AnchorState {
    pub anchor: Anchor,
    pub introduced: usize,
}

#[derive(Debug, Clone)]
pub struct ClaimState {
    pub claim: Claim,
    pub status: Status,
    /// Index of the commit that first asserted it.
    pub introduced: usize,
    /// Index of the commit that last changed its status.
    pub changed: Option<usize>,
    /// How many times it was asserted again with new evidence.
    pub reinforced: u32,
    pub replaced_by: Option<ClaimId>,
    pub reason: Option<String>,
}

impl ClaimState {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.status == Status::Active
    }
}

/// Materialized memory. Lookups by subject and by target are indexed.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub sources: BTreeMap<SourceId, SourceState>,
    pub anchors: BTreeMap<AnchorId, AnchorState>,
    pub claims: BTreeMap<ClaimId, ClaimState>,
    by_about: HashMap<String, Vec<ClaimId>>,
    by_target: HashMap<AnchorId, Vec<ClaimId>>,
}

impl State {
    /// Replay a lineage (root first). Deterministic: the same commits give the same state.
    #[must_use]
    pub fn replay(lineage: &[Commit]) -> Self {
        let mut state = Self::default();
        for (i, commit) in lineage.iter().enumerate() {
            for op in &commit.ops {
                state.apply(op, i);
            }
        }
        state
    }

    /// Apply one op as part of the commit at `idx`.
    pub fn apply(&mut self, op: &Op, idx: usize) {
        match op {
            Op::AddSource { source } => {
                self.sources
                    .entry(source.id.clone())
                    .or_insert_with(|| SourceState {
                        source: source.clone(),
                        introduced: idx,
                    });
            }
            Op::AddAnchor { anchor } => match self.anchors.get_mut(&anchor.id) {
                Some(existing) => merge_anchor(&mut existing.anchor, anchor),
                None => {
                    self.anchors.insert(
                        anchor.id.clone(),
                        AnchorState {
                            anchor: anchor.clone(),
                            introduced: idx,
                        },
                    );
                }
            },
            Op::AddClaim { claim } => self.add_claim(claim, idx),
            Op::SetStatus {
                claim,
                status,
                reason,
                replaced_by,
            } => {
                if let Some(cs) = self.claims.get_mut(claim) {
                    cs.status = *status;
                    cs.changed = Some(idx);
                    cs.reason.clone_from(reason);
                    cs.replaced_by.clone_from(replaced_by);
                }
            }
        }
    }

    fn add_claim(&mut self, claim: &Claim, idx: usize) {
        if let Some(existing) = self.claims.get_mut(&claim.id) {
            let mut added = false;
            for group in &claim.evidence {
                if !existing.claim.body.evidence.contains(group) {
                    existing.claim.body.evidence.push(group.clone());
                    added = true;
                }
            }
            if added {
                existing.reinforced += 1;
            }
            return;
        }
        self.by_about
            .entry(claim.about.as_str().to_owned())
            .or_default()
            .push(claim.id.clone());
        if let Some(target) = claim.value.anchor() {
            self.by_target
                .entry(target.clone())
                .or_default()
                .push(claim.id.clone());
        }
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

    pub fn active(&self) -> impl Iterator<Item = &ClaimState> {
        self.claims.values().filter(|c| c.is_active())
    }

    /// Every claim about `subject`, whatever its status (its history).
    pub fn history_of<'a>(&'a self, subject: &str) -> impl Iterator<Item = &'a ClaimState> + 'a {
        self.by_about
            .get(subject)
            .into_iter()
            .flatten()
            .filter_map(|id| self.claims.get(id))
    }

    /// Active claims about `subject`.
    pub fn about<'a>(&'a self, subject: &str) -> impl Iterator<Item = &'a ClaimState> + 'a {
        self.history_of(subject).filter(|c| c.is_active())
    }

    /// Active claims whose value links to `anchor`.
    pub fn pointing_to<'a>(&'a self, anchor: &str) -> impl Iterator<Item = &'a ClaimState> + 'a {
        self.by_target
            .get(anchor)
            .into_iter()
            .flatten()
            .filter_map(|id| self.claims.get(id))
            .filter(|c| c.is_active())
    }

    #[must_use]
    pub fn anchor(&self, id: &str) -> Option<&AnchorState> {
        self.anchors.get(id)
    }

    /// Whether anything active is still said about or to the anchor (retracted-only anchors fade).
    #[must_use]
    pub fn is_present(&self, anchor: &str) -> bool {
        self.about(anchor).next().is_some() || self.pointing_to(anchor).next().is_some()
    }

    /// Sources behind active observed/stated claims about `anchor`: what an inference can rest on.
    #[must_use]
    pub fn sources_supporting(&self, anchor: &str) -> Vec<SourceId> {
        supporting_sources(self.about(anchor).map(|c| &c.claim))
    }
}

fn merge_anchor(existing: &mut Anchor, incoming: &Anchor) {
    for alias in &incoming.aliases {
        if !existing.aliases.contains(alias) {
            existing.aliases.push(alias.clone());
        }
    }
    if incoming.summary.is_some() {
        existing.summary.clone_from(&incoming.summary);
    }
    if !incoming.name.is_empty() {
        existing.name.clone_from(&incoming.name);
    }
}

fn supporting_sources<'a>(claims: impl Iterator<Item = &'a Claim>) -> Vec<SourceId> {
    let mut out: Vec<SourceId> = Vec::new();
    for c in claims.filter(|c| matches!(c.basis, Basis::Observed | Basis::Stated)) {
        for s in c.evidence_sources() {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
    }
    out
}

/// Changes staged on top of a [`State`] without copying it. Lookups see base + staged.
/// Turn it into a commit with [`Batch::into_ops`].
#[derive(Debug)]
pub struct Batch<'a> {
    base: &'a State,
    staged: State,
    status: HashMap<ClaimId, Status>,
    ops: Vec<Op>,
}

impl<'a> Batch<'a> {
    #[must_use]
    pub fn new(base: &'a State) -> Self {
        Self {
            base,
            staged: State::default(),
            status: HashMap::new(),
            ops: Vec::new(),
        }
    }

    pub fn push(&mut self, op: Op) {
        if let Op::SetStatus { claim, status, .. } = &op {
            self.status.insert(claim.clone(), *status);
        }
        self.staged.apply(&op, usize::MAX);
        self.ops.push(op);
    }

    #[must_use]
    pub fn base(&self) -> &State {
        self.base
    }

    #[must_use]
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    #[must_use]
    pub fn into_ops(self) -> Vec<Op> {
        self.ops
    }

    #[must_use]
    pub fn anchor(&self, id: &str) -> Option<&Anchor> {
        self.staged
            .anchor(id)
            .or_else(|| self.base.anchor(id))
            .map(|a| &a.anchor)
    }

    #[must_use]
    pub fn has_source(&self, id: &SourceId) -> bool {
        self.staged.sources.contains_key(id) || self.base.sources.contains_key(id)
    }

    fn claim(&self, id: &ClaimId) -> Option<&ClaimState> {
        self.staged
            .claims
            .get(id)
            .or_else(|| self.base.claims.get(id))
    }

    /// Current status of a claim, seeing staged changes.
    #[must_use]
    pub fn claim_status(&self, id: &ClaimId) -> Option<Status> {
        self.status
            .get(id)
            .copied()
            .or_else(|| self.claim(id).map(|c| c.status))
    }

    /// Whether `subject` names a known anchor or claim.
    #[must_use]
    pub fn knows_subject(&self, subject: &str) -> bool {
        self.anchor(subject).is_some()
            || subject
                .parse::<ClaimId>()
                .is_ok_and(|id| self.claim(&id).is_some())
    }

    /// Active claims about `subject`, base and staged, without duplicates.
    #[must_use]
    pub fn active_about(&self, subject: &str) -> Vec<&Claim> {
        let mut out: Vec<&Claim> = Vec::new();
        for cs in self
            .base
            .history_of(subject)
            .chain(self.staged.history_of(subject))
        {
            let active = self.claim_status(&cs.claim.id) == Some(Status::Active);
            if active && !out.iter().any(|c| c.id == cs.claim.id) {
                out.push(&cs.claim);
            }
        }
        out
    }

    #[must_use]
    pub fn sources_supporting(&self, anchor: &str) -> Vec<SourceId> {
        supporting_sources(self.active_about(anchor).into_iter())
    }

    /// Assert a claim: nothing if known with the same evidence; a reinforcement if the evidence
    /// is new; for single-valued relations, supersede the previous value instead of keeping both.
    pub fn assert(&mut self, claim: Claim) {
        if let Some(status) = self.claim_status(&claim.id) {
            let known = self.claim(&claim.id).map(|c| &c.claim.evidence);
            let new_evidence = known.is_some_and(|k| claim.evidence.iter().any(|g| !k.contains(g)));
            if status == Status::Active && new_evidence {
                self.push(Op::AddClaim { claim });
            }
            return;
        }
        if claim.relation.info().functional {
            let replaced: Vec<ClaimId> = self
                .active_about(claim.about.as_str())
                .into_iter()
                .filter(|c| c.relation == claim.relation && c.scope == claim.scope)
                .map(|c| c.id.clone())
                .collect();
            for old in replaced {
                self.push(Op::SetStatus {
                    claim: old,
                    status: Status::Superseded,
                    reason: Some("newer value".to_owned()),
                    replaced_by: Some(claim.id.clone()),
                });
            }
        }
        self.push(Op::AddClaim { claim });
    }

    /// Assert only if this exact claim isn't already active: re-observing a known fact is not news.
    pub fn assert_if_new(&mut self, claim: Claim) {
        if self.claim_status(&claim.id) != Some(Status::Active) {
            self.assert(claim);
        }
    }

    /// Add an anchor if it is new or its name/summary changed.
    pub fn upsert_anchor(&mut self, anchor: Anchor) {
        let changed = match self.anchor(anchor.id.as_str()) {
            None => true,
            Some(a) => {
                (anchor.summary.is_some() && a.summary != anchor.summary) || a.name != anchor.name
            }
        };
        if changed {
            self.push(Op::AddAnchor { anchor });
        }
    }

    /// Add a source if not already known.
    pub fn add_source(&mut self, source: Source) {
        if !self.has_source(&source.id) {
            self.push(Op::AddSource { source });
        }
    }

    /// Evidence ref helper.
    #[must_use]
    pub fn evidence(source: &SourceId) -> Vec<EvidenceRef> {
        vec![EvidenceRef::source(source.clone())]
    }
}
