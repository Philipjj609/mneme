//! Comprehension frames: what understanding a kind of thing *consists of*.
//! A slot is filled by active claims of its relations; an empty slot is a named unknown (frontier).
//!
//! v0 keeps frames as versioned files in `frames/`. They move into the log once frames are learned.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::state::{ClaimState, State};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Slot {
    pub id: String,
    pub label: String,
    /// Relations that fill this slot. `in:rel` means incoming (`X rel this`).
    pub relations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    pub kind: String,
    pub label: String,
    pub slots: Vec<Slot>,
}

pub fn load_frames(dir: &Path) -> Result<Vec<Frame>> {
    let mut out = vec![];
    if !dir.exists() {
        return Ok(out);
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("json") {
            let text = fs::read_to_string(&p)?;
            let f: Frame = serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .with_context(|| format!("frame {}", p.display()))?;
            out.push(f);
        }
    }
    Ok(out)
}

pub struct SlotFill<'a> {
    pub slot: &'a Slot,
    pub claims: Vec<&'a ClaimState>,
}

pub fn slot_matches(rel_spec: &str, cs: &ClaimState, anchor: &str) -> bool {
    match rel_spec.strip_prefix("in:") {
        Some(rel) => cs.claim.relation == rel && cs.claim.value.anchor() == Some(anchor),
        None => cs.claim.relation == rel_spec && cs.claim.about == anchor,
    }
}

pub fn profile<'a>(frame: &'a Frame, state: &'a State, anchor: &str) -> Vec<SlotFill<'a>> {
    frame
        .slots
        .iter()
        .map(|slot| {
            let mut claims: Vec<&ClaimState> = state
                .active()
                .filter(|cs| slot.relations.iter().any(|r| slot_matches(r, cs, anchor)))
                .collect();
            // Stable, readable order: by relation, then by text (so "1 ·", "2 ·" read in order).
            claims.sort_by(|a, b| {
                (a.claim.relation.as_str(), a.claim.value.display()).cmp(&(b.claim.relation.as_str(), b.claim.value.display()))
            });
            SlotFill { slot, claims }
        })
        .collect()
}

pub fn coverage(fills: &[SlotFill]) -> f32 {
    if fills.is_empty() {
        return 0.0;
    }
    fills.iter().filter(|f| !f.claims.is_empty()).count() as f32 / fills.len() as f32
}
