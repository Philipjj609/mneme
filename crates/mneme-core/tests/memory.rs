//! Behaviour of the memory engine, end to end on temporary memory folders. Several tests pin
//! bugs found on real data (named in the test).

use mneme_core::frames::{profile, Frame, Slot};
use mneme_core::model::{
    Anchor, AnchorId, Basis, Claim, Op, Scope, Source, SourceId, Status, Value,
};
use mneme_core::project::{write_all, BuildInfo};
use mneme_core::{digest, recall, Batch, Error, Frames, Repo, State};
use organon_protocol::Proposal;

fn repo() -> (tempfile::TempDir, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repo::init(dir.path()).unwrap();
    (dir, repo)
}

fn id(s: &str) -> AnchorId {
    s.parse().unwrap()
}

fn anchor(i: &str, name: &str) -> Op {
    Op::AddAnchor {
        anchor: Anchor {
            id: id(i),
            name: name.into(),
            aliases: vec![],
            scope: Scope::world(),
            summary: None,
        },
    }
}

fn source(tag: &str) -> Source {
    Source {
        id: SourceId::of(tag.as_bytes()),
        kind: "test".into(),
        uri: format!("test://{tag}"),
        title: tag.into(),
        captured_at: "2026-11-05T00:00:00Z".into(),
        content_hash: organon_protocol::sha256_hex(tag.as_bytes()),
        files: Default::default(),
        meta: Default::default(),
        produced_by: None,
    }
}

fn claim(about: &str, rel: &str, value: Value, basis: Basis, src: &Source) -> Claim {
    Claim::draft(id(about), rel, value, basis, "test")
        .with_evidence(src.id.clone())
        .finalize()
}

fn link(to: &str) -> Value {
    Value::Anchor(id(to))
}

fn state(repo: &Repo) -> State {
    repo.state_at(None).unwrap().1
}

fn proposal(json: serde_json::Value) -> Proposal {
    serde_json::from_value(json).unwrap()
}

#[test]
fn init_writes_frames_and_discover_finds_the_memory_from_below() {
    let (dir, _) = repo();
    assert!(dir.path().join("frames/repo.json").exists());
    let deep = dir.path().join("a/b");
    std::fs::create_dir_all(&deep).unwrap();
    assert_eq!(Repo::discover(&deep).unwrap().root(), dir.path());
    assert!(matches!(
        Repo::discover(tempfile::tempdir().unwrap().path()),
        Err(Error::NoMemory(_))
    ));
}

#[test]
fn empty_commit_is_refused() {
    let (_d, repo) = repo();
    assert!(matches!(
        repo.commit("t", "nothing", vec![]),
        Err(Error::NothingToCommit)
    ));
}

#[test]
fn replay_is_deterministic_and_commits_chain() {
    let (_d, repo) = repo();
    let s = source("a");
    let c1 = repo
        .commit(
            "t",
            "one",
            vec![
                Op::AddSource { source: s.clone() },
                anchor("repo:a/b", "a/b"),
            ],
        )
        .unwrap();
    let c2 = repo
        .commit(
            "t",
            "two",
            vec![Op::AddClaim {
                claim: claim(
                    "repo:a/b",
                    "language",
                    Value::Text("Rust".into()),
                    Basis::Observed,
                    &s,
                ),
            }],
        )
        .unwrap();
    assert_eq!(c2.parents, vec![c1.id.clone()]);
    let (lineage, a) = repo.state_at(None).unwrap();
    let b = State::replay(&lineage);
    assert_eq!(a.claims.len(), b.claims.len());
    assert_eq!(
        a.anchors.keys().collect::<Vec<_>>(),
        b.anchors.keys().collect::<Vec<_>>()
    );
}

#[test]
fn single_valued_relation_supersedes_and_keeps_history() {
    let (_d, repo) = repo();
    let s1 = source("day1");
    let s2 = source("day2");
    let mut ops = vec![
        Op::AddSource { source: s1.clone() },
        Op::AddSource { source: s2.clone() },
        anchor("repo:a/b", "a/b"),
    ];
    let mut old = Claim::draft(
        id("repo:a/b"),
        "stars",
        Value::Number(10.0),
        Basis::Observed,
        "t",
    )
    .with_evidence(s1.id.clone());
    old.valid_from = Some("2026-11-04".into());
    ops.push(Op::AddClaim {
        claim: old.finalize(),
    });
    repo.commit("t", "seed", ops).unwrap();

    let st = state(&repo);
    let mut batch = Batch::new(&st);
    let mut new = Claim::draft(
        id("repo:a/b"),
        "stars",
        Value::Number(12.0),
        Basis::Observed,
        "t",
    )
    .with_evidence(s2.id.clone());
    new.valid_from = Some("2026-11-05".into());
    batch.assert(new.finalize());
    let ops = batch.into_ops();
    assert!(
        matches!(
            ops[0],
            Op::SetStatus {
                status: Status::Superseded,
                ..
            }
        ),
        "{ops:?}"
    );
    repo.commit("t", "recapture", ops).unwrap();

    let st = state(&repo);
    let active: Vec<String> = st
        .about("repo:a/b")
        .map(|c| c.claim.value.to_string())
        .collect();
    assert_eq!(active, ["12"]);
    assert_eq!(
        st.history_of("repo:a/b").count(),
        2,
        "old value stays in history"
    );
}

#[test]
fn same_statement_twice_is_one_claim_and_new_evidence_reinforces_it() {
    let (_d, repo) = repo();
    let (s1, s2) = (source("one"), source("two"));
    let c1 = claim(
        "repo:a/b",
        "provides",
        link("concept:x"),
        Basis::Inferred,
        &s1,
    );
    repo.commit(
        "t",
        "seed",
        vec![
            Op::AddSource { source: s1.clone() },
            Op::AddSource { source: s2.clone() },
            anchor("repo:a/b", "a/b"),
            anchor("concept:x", "X"),
            Op::AddClaim { claim: c1.clone() },
        ],
    )
    .unwrap();

    let st = state(&repo);
    let mut batch = Batch::new(&st);
    batch.assert(c1.clone());
    assert!(
        batch.ops().is_empty(),
        "same claim, same evidence: nothing to do"
    );
    let again = claim(
        "repo:a/b",
        "provides",
        link("concept:x"),
        Basis::Inferred,
        &s2,
    );
    assert_eq!(again.id, c1.id, "identity ignores evidence");
    batch.assert(again);
    repo.commit("t", "reinforce", batch.into_ops()).unwrap();
    assert_eq!(state(&repo).claims[&c1.id].reinforced, 1);
}

#[test]
fn retracted_claims_leave_the_active_view_and_lone_anchors_fade() {
    let (_d, repo) = repo();
    let s = source("s");
    let c = claim(
        "site:x.com/i/article/1",
        "title",
        Value::Text("X".into()),
        Basis::Observed,
        &s,
    );
    repo.commit(
        "t",
        "seed",
        vec![
            Op::AddSource { source: s },
            anchor("site:x.com/i/article/1", "shell"),
            Op::AddClaim { claim: c.clone() },
        ],
    )
    .unwrap();
    assert!(state(&repo).is_present("site:x.com/i/article/1"));
    repo.commit(
        "t",
        "retract",
        vec![Op::SetStatus {
            claim: c.id.clone(),
            status: Status::Retracted,
            reason: Some("duplicate".into()),
            replaced_by: None,
        }],
    )
    .unwrap();
    let st = state(&repo);
    assert_eq!(st.about("site:x.com/i/article/1").count(), 0);
    assert!(!st.is_present("site:x.com/i/article/1"));
    assert_eq!(st.claims[&c.id].reason.as_deref(), Some("duplicate"));
}

#[test]
fn branches_isolate_changes_and_revisions_resolve() {
    let (_d, repo) = repo();
    let first = repo
        .commit("t", "base", vec![anchor("concept:a", "A")])
        .unwrap();
    repo.create_branch("what-if", None).unwrap();
    repo.switch("what-if").unwrap();
    repo.commit("t", "idea", vec![anchor("concept:b", "B")])
        .unwrap();
    repo.switch("main").unwrap();
    assert!(
        state(&repo).anchor("concept:b").is_none(),
        "main doesn't see the what-if"
    );
    assert!(repo
        .state_at(Some("what-if"))
        .unwrap()
        .1
        .anchor("concept:b")
        .is_some());
    assert_eq!(repo.resolve(&first.id.as_str()[..8]).unwrap(), first.id);
    assert!(matches!(
        repo.resolve("m_"),
        Err(Error::AmbiguousRevision { .. })
    ));
    assert!(matches!(
        repo.create_branch("what-if", None),
        Err(Error::BranchExists(_))
    ));
    assert!(matches!(repo.switch("nope"), Err(Error::NoSuchBranch(_))));
}

#[test]
fn log_written_on_windows_with_bom_and_crlf_still_reads() {
    let (dir, repo) = repo();
    let c = repo
        .commit("t", "base", vec![anchor("concept:a", "A")])
        .unwrap();
    let log = dir.path().join("memory/log.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    std::fs::write(&log, format!("\u{feff}{}", text.replace('\n', "\r\n"))).unwrap();
    assert_eq!(repo.commits().unwrap()[0].id, c.id);
}

fn seeded() -> (tempfile::TempDir, Repo, Source) {
    let (d, repo) = repo();
    let s = source("readme");
    repo.commit(
        "capture",
        "seed",
        vec![
            Op::AddSource { source: s.clone() },
            anchor("repo:a/b", "a/b"),
            anchor("concept:motion-design", "Motion design"),
            Op::AddClaim {
                claim: claim(
                    "repo:a/b",
                    "described_as",
                    Value::Text("A video tool".into()),
                    Basis::Stated,
                    &s,
                ),
            },
        ],
    )
    .unwrap();
    (d, repo, s)
}

fn digest_ops(repo: &Repo, p: serde_json::Value) -> Result<Vec<Op>, Error> {
    digest::ops_from_proposal(&state(repo), &proposal(p), source("proposal"))
}

#[test]
fn digest_links_known_ids_keeps_free_text_and_attaches_evidence() {
    let (_d, repo, readme) = seeded();
    let ops = digest_ops(&repo, serde_json::json!({
        "author": "model", "message": "m", "evidence": "auto",
        "claims": [["repo:a/b", "provides", "concept:motion-design", 0.8, "from README"],
                   {"about": "repo:a/b", "relation": "mechanism", "value": "renders frames: as code", "basis": "stated", "confidence": 0.9}]
    }))
    .unwrap();
    let claims: Vec<&Claim> = ops
        .iter()
        .filter_map(|o| {
            if let Op::AddClaim { claim } = o {
                Some(claim)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(claims[0].value, link("concept:motion-design"));
    assert_eq!(claims[0].confidence, Some(0.8));
    assert_eq!(
        claims[0].evidence_sources().next(),
        Some(&readme.id),
        "auto evidence cites the capture source"
    );
    assert_eq!(
        claims[1].value,
        Value::Text("renders frames: as code".into())
    );
    assert_eq!(
        claims[1].confidence, None,
        "stated claims carry no confidence"
    );
    assert!(
        ops.iter().any(
            |o| matches!(o, Op::AddSource { source } if source.id == SourceId::of(b"proposal"))
        ),
        "proposal archived"
    );
}

#[test]
fn digest_rejects_unknown_subjects_and_id_like_typos() {
    // Real bug: `concept:motion-design` was used before it existed and silently became text.
    let (_d, repo, _) = seeded();
    let err = digest_ops(&repo, serde_json::json!({
        "author": "m", "message": "m",
        "claims": [["repo:nope/x", "provides", "concept:motion-design"], ["repo:a/b", "provides", "concept:motion-desing"]]
    }))
    .unwrap_err();
    let Error::Rejected(errors) = err else {
        panic!("expected Rejected")
    };
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("unknown subject"));
    assert!(errors[1].contains("looks like an anchor id"));
}

#[test]
fn digest_sees_anchors_proposed_in_the_same_file() {
    let (_d, repo, _) = seeded();
    let ops = digest_ops(
        &repo,
        serde_json::json!({
            "author": "m", "message": "m",
            "anchors": [{"id": "concept:new-idea", "name": "New idea"}],
            "claims": [["concept:new-idea", "part_of", "concept:motion-design"]]
        }),
    )
    .unwrap();
    assert!(ops.iter().any(
        |o| matches!(o, Op::AddClaim { claim } if claim.about.as_str() == "concept:new-idea")
    ));
}

#[test]
fn recall_ranks_the_exact_concept_above_a_hub() {
    // Real bug: "motion design" returned the design-inspiration hub first, because activation
    // summed over its many incoming links.
    let (_d, repo) = repo();
    let s = source("s");
    let mut ops = vec![
        Op::AddSource { source: s.clone() },
        anchor("concept:motion-design", "Motion design"),
        anchor("concept:design-inspiration", "Design inspiration"),
    ];
    ops.push(Op::AddClaim {
        claim: claim(
            "concept:motion-design",
            "defined_as",
            Value::Text("animation rendered from code".into()),
            Basis::Inferred,
            &s,
        ),
    });
    for i in 0..30 {
        let site = format!("site:gallery{i}.design");
        ops.push(anchor(&site, &format!("gallery{i}.design")));
        ops.push(Op::AddClaim {
            claim: claim(
                &site,
                "described_as",
                Value::Text("design gallery".into()),
                Basis::Stated,
                &s,
            ),
        });
        ops.push(Op::AddClaim {
            claim: claim(
                &site,
                "provides",
                link("concept:design-inspiration"),
                Basis::Inferred,
                &s,
            ),
        });
    }
    repo.commit("t", "seed", ops).unwrap();
    let r = recall::recall(&state(&repo), "motion design", 5);
    assert_eq!(
        r.items[0].id.as_str(),
        "concept:motion-design",
        "{:?}",
        r.items
    );
}

#[test]
fn recall_does_not_confuse_engineer_with_engine() {
    let (_d, repo) = repo();
    let s = source("s");
    repo.commit(
        "t",
        "seed",
        vec![
            Op::AddSource { source: s.clone() },
            anchor("concept:reverse-engineering", "Reverse engineering"),
            anchor("repo:x/search", "x/search"),
            Op::AddClaim {
                claim: claim(
                    "repo:x/search",
                    "described_as",
                    Value::Text("AI search engine".into()),
                    Basis::Stated,
                    &s,
                ),
            },
            Op::AddClaim {
                claim: claim(
                    "concept:reverse-engineering",
                    "defined_as",
                    Value::Text("understand binaries".into()),
                    Basis::Inferred,
                    &s,
                ),
            },
        ],
    )
    .unwrap();
    let r = recall::recall(&state(&repo), "how to reverse engineer an app", 5);
    assert!(
        r.items.iter().all(|i| i.id.as_str() != "repo:x/search"),
        "{:?}",
        r.items
    );
}

#[test]
fn frames_fill_slots_from_incoming_links_too() {
    let (_d, repo) = repo();
    let s = source("s");
    repo.commit(
        "t",
        "seed",
        vec![
            Op::AddSource { source: s.clone() },
            anchor("concept:c", "C"),
            anchor("repo:a/b", "a/b"),
            Op::AddClaim {
                claim: claim(
                    "repo:a/b",
                    "provides",
                    link("concept:c"),
                    Basis::Inferred,
                    &s,
                ),
            },
        ],
    )
    .unwrap();
    let frame = Frame {
        kind: "concept".into(),
        label: "Concept".into(),
        slots: vec![
            Slot {
                id: "realizations".into(),
                label: "Realizations".into(),
                relations: vec!["in:provides".into()],
                hint: None,
            },
            Slot {
                id: "definition".into(),
                label: "Definition".into(),
                relations: vec!["defined_as".into()],
                hint: None,
            },
        ],
    };
    let st = state(&repo);
    let fills = profile(&frame, &st, "concept:c");
    assert_eq!(fills[0].claims.len(), 1);
    assert!(fills[1].is_empty(), "an empty slot is a named unknown");
}

#[test]
fn projections_build_and_escape_script_endings() {
    let (dir, repo) = repo();
    let s = source("s");
    repo.commit(
        "t",
        "seed",
        vec![
            Op::AddSource { source: s.clone() },
            anchor("concept:c", "C"),
            Op::AddClaim {
                claim: claim(
                    "concept:c",
                    "defined_as",
                    Value::Text("evil </script><b>".into()),
                    Basis::Inferred,
                    &s,
                ),
            },
        ],
    )
    .unwrap();
    let (lineage, st) = repo.state_at(None).unwrap();
    let frames = Frames::load(&repo.frames_dir()).unwrap();
    let refs = repo.refs().unwrap();
    let out = dir.path().join("projections");
    write_all(
        &out,
        &lineage,
        &st,
        &frames,
        BuildInfo {
            branch: &refs.head,
            branches: &refs.branches,
        },
    )
    .unwrap();
    let html = std::fs::read_to_string(out.join("view.html")).unwrap();
    assert!(
        !html.contains("evil </script>"),
        "data must not close the script tag"
    );
    assert!(html.contains("evil <\\/script>"));
    assert!(out.join("wiki/concept/c.md").exists() && out.join("wiki/index.md").exists());
}
