//! Relation registry. Unknown relations are allowed (open world) and fall into family `other`.

use crate::model::Stance;

pub struct RelInfo {
    /// Edge layer, used for toggles in views ("meta", "capability", "peer", ...).
    pub family: &'static str,
    /// Single-valued: a new value supersedes the old one instead of coexisting.
    pub functional: bool,
    pub stance: Stance,
}

pub fn info(rel: &str) -> RelInfo {
    let (family, functional, stance) = match rel {
        "described_as" | "homepage" | "license" | "language" | "stars" | "owned_by" => {
            ("meta", true, Stance::Is)
        }
        "tagged" => ("topic", false, Stance::Is),
        "starred_by" | "saved_by" => ("personal", false, Stance::Is),
        "provides" | "implements" => ("capability", false, Stance::Is),
        "part_of" | "instance_of" => ("taxonomy", false, Stance::Is),
        "alternative_to" | "complements" => ("peer", false, Stance::Is),
        "builds_on" | "inspired_by" => ("lineage", false, Stance::Is),
        "relevant_to" => ("use", false, Stance::Could),
        "uses" | "needs" | "used_in" => ("use", false, Stance::Is),
        "defined_as" | "purpose" | "mechanism" => ("meaning", false, Stance::Is),
        "limitation" | "failure_mode" | "constraint" => ("bounds", false, Stance::Is),
        "question" => ("frontier", false, Stance::Is),
        "status" => ("state", true, Stance::Is),
        _ => ("other", false, Stance::Is),
    };
    RelInfo { family, functional, stance }
}

pub const FAMILIES: &[&str] = &[
    "capability", "taxonomy", "peer", "lineage", "use", "meaning", "bounds", "topic", "meta",
    "personal", "frontier", "state", "other",
];
