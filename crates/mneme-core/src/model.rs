//! What the log stores. Knowledge objects (sources, anchors, claims) are organon-protocol
//! types; ops, commits and refs are mneme's own: history is the memory's concern.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use organon_protocol::{
    Anchor, AnchorId, Basis, Claim, ClaimBody, ClaimId, CommitId, EvidenceRef, Family, Relation,
    Scope, Source, SourceId, Stance, Status, Subject, Value,
};

/// One change to the memory. A commit is an ordered list of ops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Op {
    AddSource {
        source: Source,
    },
    AddAnchor {
        anchor: Anchor,
    },
    /// Asserting a known claim again with new evidence reinforces it.
    AddClaim {
        claim: Claim,
    },
    SetStatus {
        claim: ClaimId,
        status: Status,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaced_by: Option<ClaimId>,
    },
}

/// A content-addressed batch of ops chained to its parent, like a git commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub id: CommitId,
    pub parents: Vec<CommitId>,
    pub author: String,
    pub message: String,
    /// RFC 3339: when the memory learned this (world time lives on claims).
    pub time: String,
    pub ops: Vec<Op>,
}

impl Commit {
    #[must_use]
    pub fn new(
        parents: Vec<CommitId>,
        author: &str,
        message: &str,
        time: String,
        ops: Vec<Op>,
    ) -> Self {
        let body = serde_json::json!([parents, author, message, time, ops]);
        Self {
            id: CommitId::of(body.to_string().as_bytes()),
            parents,
            author: author.to_owned(),
            message: message.to_owned(),
            time,
            ops,
        }
    }
}

/// Branch heads and the current branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refs {
    pub head: String,
    pub branches: BTreeMap<String, CommitId>,
}

impl Default for Refs {
    fn default() -> Self {
        Self {
            head: "main".to_owned(),
            branches: BTreeMap::new(),
        }
    }
}
