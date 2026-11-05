use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::util::short_hash;

pub type Id = String;

/// Where a claim's knowledge comes from.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Basis {
    /// A tool observed it in a source (API response, file, parse).
    Observed,
    /// Someone said it (an author, a README, Philip).
    Stated,
    /// A model or person reasoned to it. Carries confidence + rationale.
    Inferred,
    /// Taken from another knowledge system wholesale.
    Imported,
    /// An intuition, kept as intuition.
    Intuited,
}

impl Basis {
    pub fn as_str(&self) -> &'static str {
        match self {
            Basis::Observed => "observed",
            Basis::Stated => "stated",
            Basis::Inferred => "inferred",
            Basis::Imported => "imported",
            Basis::Intuited => "intuited",
        }
    }
}

/// Whether a claim describes reality, an intention, a possibility, or a fiction.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Stance {
    #[default]
    Is,
    Should,
    Could,
    Canon,
    Hypothetical,
}

impl Stance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Stance::Is => "is",
            Stance::Should => "should",
            Stance::Could => "could",
            Stance::Canon => "canon",
            Stance::Hypothetical => "hypothetical",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Superseded,
    Contradicted,
    Parked,
    Retracted,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Superseded => "superseded",
            Status::Contradicted => "contradicted",
            Status::Parked => "parked",
            Status::Retracted => "retracted",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Value {
    Anchor(Id),
    Text(String),
    Number(f64),
    Bool(bool),
}

impl Value {
    pub fn anchor(&self) -> Option<&str> {
        match self {
            Value::Anchor(a) => Some(a),
            _ => None,
        }
    }
    pub fn display(&self) -> String {
        match self {
            Value::Anchor(a) => a.clone(),
            Value::Text(t) => t.clone(),
            Value::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    format!("{n}")
                }
            }
            Value::Bool(b) => b.to_string(),
        }
    }
}

/// Immutable evidence: a file, URL, API response, conversation, proposal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: Id,
    pub kind: String,
    pub uri: String,
    pub title: String,
    pub captured_at: String,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, serde_json::Value>,
}

/// Identity only. All meaning about an anchor lives in claims.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Anchor {
    pub id: Id,
    #[serde(default)]
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

pub fn default_scope() -> String {
    "world".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvidenceRef {
    pub source: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<String>,
}

/// The atom: a qualified statement about an anchor (or about another claim).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claim {
    pub id: Id,
    pub about: Id,
    pub relation: String,
    pub value: Value,
    #[serde(default = "default_scope")]
    pub scope: String,
    /// When this became true in the world (not when we learned it — that is the commit time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    pub basis: Basis,
    #[serde(default)]
    pub stance: Stance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// Groups of evidence: every ref in a group is jointly required; groups are alternatives.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Vec<EvidenceRef>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<Id>,
    pub author: String,
}

impl Claim {
    pub fn new(about: &str, relation: &str, value: Value, basis: Basis, author: &str) -> Claim {
        Claim {
            id: String::new(),
            about: about.into(),
            relation: relation.into(),
            value,
            scope: default_scope(),
            valid_from: None,
            valid_to: None,
            basis,
            stance: crate::relations::info(relation).stance,
            confidence: None,
            rationale: None,
            evidence: vec![],
            derived_from: vec![],
            author: author.into(),
        }
    }

    /// Identity of a claim is its content, not who said it or why:
    /// the same statement asserted twice is one claim with more evidence.
    pub fn compute_id(&self) -> Id {
        let key = serde_json::json!([
            self.about,
            self.relation,
            self.value,
            self.scope,
            self.valid_from,
            self.stance
        ]);
        short_hash("c_", key.to_string().as_bytes())
    }

    pub fn finalize(mut self) -> Claim {
        self.id = self.compute_id();
        self
    }

    pub fn with_evidence(mut self, source: &str) -> Claim {
        self.evidence.push(vec![EvidenceRef { source: source.into(), span: None }]);
        self
    }

    pub fn evidence_sources(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.evidence.iter().flatten().map(|e| e.source.as_str()).collect();
        v.dedup();
        v
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    AddSource {
        source: Source,
    },
    AddAnchor {
        anchor: Anchor,
    },
    AddClaim {
        claim: Claim,
    },
    SetStatus {
        claim: Id,
        status: Status,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaced_by: Option<Id>,
    },
}

/// A content-addressed batch of ops, chained to its parent like a git commit.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub id: Id,
    pub parents: Vec<Id>,
    pub author: String,
    pub message: String,
    pub time: String,
    pub ops: Vec<Op>,
}

impl Commit {
    pub fn new(parents: Vec<Id>, author: &str, message: &str, time: String, ops: Vec<Op>) -> Commit {
        let body = serde_json::json!([parents, author, message, time, ops]);
        Commit {
            id: short_hash("m_", body.to_string().as_bytes()),
            parents,
            author: author.into(),
            message: message.into(),
            time,
            ops,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Refs {
    pub head: String,
    pub branches: BTreeMap<String, Id>,
}

impl Default for Refs {
    fn default() -> Self {
        Refs { head: "main".into(), branches: BTreeMap::new() }
    }
}
