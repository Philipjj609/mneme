//! Comprehension frames: what understanding a kind of thing consists of. A slot is filled by
//! active claims of its relations; an empty slot is a named unknown (the frontier).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::state::{ClaimState, State};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub id: String,
    pub label: String,
    /// Relations that fill this slot; `in:rel` means incoming (`X rel this`).
    pub relations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Slot {
    fn outgoing(&self) -> impl Iterator<Item = &str> {
        self.relations
            .iter()
            .map(String::as_str)
            .filter(|r| !r.starts_with("in:"))
    }
    fn incoming(&self) -> impl Iterator<Item = &str> {
        self.relations.iter().filter_map(|r| r.strip_prefix("in:"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub kind: String,
    pub label: String,
    pub slots: Vec<Slot>,
}

/// The frames of a memory, by kind.
#[derive(Debug, Clone, Default)]
pub struct Frames(Vec<Frame>);

impl Frames {
    /// Load every `*.json` in `dir` (sorted by name). A missing folder means no frames.
    ///
    /// # Errors
    /// I/O failure or a malformed frame file.
    pub fn load(dir: &Path) -> Result<Self> {
        if !dir.exists() {
            return Ok(Self::default());
        }
        let mut paths: Vec<_> = fs::read_dir(dir)
            .map_err(Error::io(dir))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let text = fs::read_to_string(&path).map_err(Error::io(&path))?;
                serde_json::from_str(text.trim_start_matches('\u{feff}'))
                    .map_err(|source| Error::Json { path, source })
            })
            .collect::<Result<Vec<Frame>>>()
            .map(Self)
    }

    #[must_use]
    pub fn new(frames: Vec<Frame>) -> Self {
        Self(frames)
    }

    #[must_use]
    pub fn for_kind(&self, kind: &str) -> Option<&Frame> {
        self.0.iter().find(|f| f.kind == kind)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Frame] {
        &self.0
    }
}

/// A slot with the claims that fill it.
#[derive(Debug)]
pub struct SlotFill<'a> {
    pub slot: &'a Slot,
    pub claims: Vec<&'a ClaimState>,
}

impl SlotFill<'_> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }
}

/// How the anchor fills its frame, slot by slot. Claims are in a stable, readable order
/// (by relation, then by text, so numbered mechanisms read 1, 2, 3).
#[must_use]
pub fn profile<'a>(frame: &'a Frame, state: &'a State, anchor: &str) -> Vec<SlotFill<'a>> {
    frame
        .slots
        .iter()
        .map(|slot| {
            let mut claims: Vec<&ClaimState> = state
                .about(anchor)
                .filter(|cs| slot.outgoing().any(|r| r == cs.claim.relation.as_str()))
                .chain(
                    state
                        .pointing_to(anchor)
                        .filter(|cs| slot.incoming().any(|r| r == cs.claim.relation.as_str())),
                )
                .collect();
            claims.sort_by_cached_key(|cs| {
                (
                    cs.claim.relation.as_str().to_owned(),
                    cs.claim.value.to_string(),
                    cs.claim.about.as_str().to_owned(),
                )
            });
            claims.dedup_by(|a, b| a.claim.id == b.claim.id);
            SlotFill { slot, claims }
        })
        .collect()
}

/// Fraction of slots with at least one claim.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn coverage(fills: &[SlotFill]) -> f32 {
    if fills.is_empty() {
        return 0.0;
    }
    fills.iter().filter(|f| !f.is_empty()).count() as f32 / fills.len() as f32
}
