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
    digest::ops_from_proposal(
        &state(repo),
        &proposal(p),
        source("proposal"),
        &repo.observation_ids().unwrap(),
    )
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

fn video_messages() -> (
    Source,
    organon_protocol::Observation,
    Vec<organon_protocol::Message>,
) {
    use organon_protocol::{Location, Message, Observation, ObservationKind, ProducedBy};
    let src = source("video");
    let said = Observation::new(
        src.id.clone(),
        ObservationKind::new(ObservationKind::TRANSCRIPT_SEGMENT),
        Location::Time { t0: 3.0, t1: 9.0 },
        serde_json::json!({"text": "depend on abstractions"}),
        ProducedBy {
            organ: "organon-video".into(),
            version: "0".into(),
            tool: None,
            model: None,
        },
    );
    let Op::AddAnchor { anchor: a } = anchor("video:yt/x", "A talk") else {
        unreachable!()
    };
    let messages = vec![
        Message::Observation(said.clone()), // order does not matter: sources are taken first
        Message::Source(src.clone()),
        Message::Anchor(a),
        Message::Claim(claim(
            "video:yt/x",
            "title",
            Value::Text("A talk".into()),
            Basis::Observed,
            &src,
        )),
    ];
    (src, said, messages)
}

#[test]
fn ingest_takes_sources_anchors_observed_claims_and_observations() {
    let (_d, repo) = repo();
    let (_, said, messages) = video_messages();
    let out = mneme_core::ingest::ops_from_messages(&state(&repo), messages, &Default::default())
        .unwrap();
    assert_eq!(out.ops.len(), 3, "source, anchor, claim");
    assert_eq!(out.observations, std::slice::from_ref(&said));
    repo.commit("organon-video", "ingest", out.ops).unwrap();
    assert_eq!(repo.save_observations(&out.observations).unwrap(), 1);
    assert_eq!(
        repo.save_observations(&out.observations).unwrap(),
        0,
        "stored once"
    );
    assert!(repo.observation_ids().unwrap().contains(&said.id));
}

#[test]
fn ingest_rejects_inferences_tampered_ids_and_dangling_evidence() {
    use organon_protocol::{EvidenceRef, Message, ObservationId};
    let (_d, repo) = repo();
    let (src, mut said, mut messages) = video_messages();
    // An inference: must go through digest.
    messages.push(Message::Claim(claim(
        "video:yt/x",
        "teaches",
        link("video:yt/x"),
        Basis::Inferred,
        &src,
    )));
    // A claim whose content was edited after its id was computed.
    let mut tampered = claim(
        "video:yt/x",
        "duration",
        Value::Number(10.0),
        Basis::Observed,
        &src,
    );
    tampered.body.value = Value::Number(11.0);
    messages.push(Message::Claim(tampered));
    // Evidence pointing at an observation nobody has.
    let mut body = Claim::draft(
        id("video:yt/x"),
        "channel",
        Value::Text("c".into()),
        Basis::Observed,
        "t",
    );
    body.evidence.push(vec![EvidenceRef::Observation {
        observation: ObservationId::of(b"nothing"),
    }]);
    messages.push(Message::Claim(body.finalize()));
    // An observation edited after its id was computed.
    said.value = serde_json::json!({"text": "edited"});
    messages.push(Message::Observation(said));
    let Err(Error::Rejected(errors)) =
        mneme_core::ingest::ops_from_messages(&state(&repo), messages, &Default::default())
    else {
        panic!("should reject")
    };
    let all = errors.join("\n");
    assert_eq!(errors.len(), 4, "{all}");
    assert!(
        all.contains("interpretation")
            && all.contains("id does not match")
            && all.contains("unknown evidence observation"),
        "{all}"
    );
}

#[test]
fn digest_cites_stored_observations_and_rejects_unknown_ones() {
    let (_d, repo) = repo();
    let (_, said, messages) = video_messages();
    let out = mneme_core::ingest::ops_from_messages(&state(&repo), messages, &Default::default())
        .unwrap();
    repo.commit("organon-video", "ingest", out.ops).unwrap();
    repo.save_observations(&out.observations).unwrap();
    let cite = |evidence: &str| {
        digest_ops(
            &repo,
            serde_json::json!({
                "author": "model", "message": "m", "basis": "inferred",
                "claims": [{"about": "video:yt/x", "relation": "described_as", "value": "a talk on dependencies",
                            "evidence": [evidence], "confidence": 0.8}]
            }),
        )
    };
    let ops = cite(said.id.as_str()).unwrap();
    let cited = ops.iter().any(|op| matches!(op, Op::AddClaim { claim }
        if claim.evidence.iter().flatten().any(|e| matches!(e, organon_protocol::EvidenceRef::Observation { observation } if *observation == said.id))));
    assert!(cited, "{ops:?}");
    assert!(matches!(
        cite("o_00000000000000ff"),
        Err(Error::Rejected(_))
    ));
}

/// Regression: videos were missing from the closed list of result kinds, so a video titled
/// "Dependency Inversion" never came back for "dependency inversion" (found on the first one).
#[test]
fn recall_returns_videos() {
    let (_d, repo) = repo();
    let s = source("s");
    repo.commit(
        "t",
        "seed",
        vec![
            Op::AddSource { source: s.clone() },
            anchor("video:yt/x", "SOLID - Dependency Inversion"),
            Op::AddClaim {
                claim: claim(
                    "video:yt/x",
                    "title",
                    Value::Text("SOLID - Dependency Inversion".into()),
                    Basis::Observed,
                    &s,
                ),
            },
            Op::AddClaim {
                claim: claim(
                    "video:yt/x",
                    "described_as",
                    Value::Text("a talk on inverting dependencies".into()),
                    Basis::Inferred,
                    &s,
                ),
            },
        ],
    )
    .unwrap();
    let r = recall::recall(&state(&repo), "dependency inversion", 5);
    assert!(
        r.items.iter().any(|i| i.id.as_str() == "video:yt/x"),
        "{:?}",
        r.items
    );
    // Organs create anchors without summaries: the brief falls back to the gist claim.
    let frames = Frames::load(&repo.frames_dir()).unwrap();
    let brief = recall::render_brief(&state(&repo), &frames, &r, "head").markdown;
    assert!(
        brief.contains("a talk on inverting dependencies"),
        "{brief}"
    );
}

#[test]
fn learned_priors_rerank_within_bounds() {
    let (_d, repo) = repo();
    let s = source("s");
    repo.commit(
        "t",
        "seed",
        vec![
            Op::AddSource { source: s.clone() },
            anchor("concept:agent-memory", "Agent memory"),
            anchor("concept:memory-palace", "Memory palace"),
            Op::AddClaim {
                claim: claim(
                    "concept:agent-memory",
                    "defined_as",
                    Value::Text("what an agent keeps between sessions".into()),
                    Basis::Inferred,
                    &s,
                ),
            },
            Op::AddClaim {
                claim: claim(
                    "concept:memory-palace",
                    "defined_as",
                    Value::Text("a mnemonic of imagined places".into()),
                    Basis::Inferred,
                    &s,
                ),
            },
        ],
    )
    .unwrap();
    let st = state(&repo);
    let plain = recall::recall(&st, "memory", 5);
    assert_eq!(plain.items.len(), 2, "{:?}", plain.items);
    let first = plain.items[0].id.to_string();
    let second = plain.items[1].id.to_string();
    let priors: recall::Priors = [(first.clone(), -0.5), (second.clone(), 0.5)].into();
    let learned = recall::recall_with(&st, "memory", 5, &priors);
    assert_eq!(
        learned.items[0].id.as_str(),
        second,
        "a useful anchor rises"
    );
    let wild: recall::Priors = [(first.clone(), -100.0)].into();
    let clamped = recall::recall_with(&st, "memory", 5, &wild);
    let score = clamped
        .items
        .iter()
        .find(|i| i.id.as_str() == first)
        .unwrap()
        .score;
    assert!(
        (score - plain.items[0].score * 0.5).abs() < 1e-6,
        "priors are clamped to ±0.5"
    );
}

#[test]
fn ingest_placeholder_anchors_never_rename_known_ones() {
    // Regression: organon-text names linked things by id only ("x/u" for account:x/u); ingesting
    // that must not overwrite "@u" that memory already has.
    use organon_protocol::Message;
    let (_d, repo) = repo();
    repo.commit("t", "seed", vec![anchor("account:x/u", "@u")])
        .unwrap();
    let Op::AddAnchor { anchor: stub } = anchor("account:x/u", "x/u") else {
        unreachable!()
    };
    let Op::AddAnchor { anchor: fresh } = anchor("site:new.dev", "new.dev") else {
        unreachable!()
    };
    let out = mneme_core::ingest::ops_from_messages(
        &state(&repo),
        vec![Message::Anchor(stub), Message::Anchor(fresh)],
        &Default::default(),
    )
    .unwrap();
    assert_eq!(out.ops.len(), 1, "only the unknown placeholder is added");
}

/// Regression (probe of `project:organon`): a slot with 15 decisions always showed the same 8,
/// the first in alphabetical order, whatever the question and however large the budget.
fn project_with_decisions() -> (tempfile::TempDir, Repo) {
    project_with(0)
}

/// The 15 decisions plus `questions` open questions (a second slot of the same item).
fn project_with(questions: usize) -> (tempfile::TempDir, Repo) {
    let (d, repo) = repo();
    let s = source("s");
    let mut ops = vec![
        Op::AddSource { source: s.clone() },
        anchor("project:organon", "Organon"),
    ];
    // The topics sort alphabetically, so "zebra" is far outside the first 8.
    let topics = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet", "kilo", "lima", "mike", "november", "zebra",
    ];
    for (i, topic) in topics.iter().enumerate() {
        ops.push(Op::AddClaim {
            claim: claim(
                "project:organon",
                "decision",
                Value::Text(format!("{topic} rule: settled on option {i} for {topic}")),
                Basis::Stated,
                &s,
            ),
        });
    }
    for i in 0..questions {
        ops.push(Op::AddClaim {
            claim: claim(
                "project:organon",
                "question",
                Value::Text(format!("riddle {i:02}: is mystery {i:02} still open")),
                Basis::Stated,
                &s,
            ),
        });
    }
    repo.commit("t", "seed", ops).unwrap();
    (d, repo)
}

fn recall_text(repo: &Repo, question: &str, budget: usize) -> String {
    let frames = Frames::load(&repo.frames_dir()).unwrap();
    let st = state(repo);
    let r = recall::recall(&st, question, 5);
    recall::render(&st, &frames, &r, "head", budget).markdown
}

#[test]
fn recall_shows_the_claims_most_relevant_to_the_question_first() {
    let (_d, repo) = project_with_decisions();
    // Tight budget: only the minimum per slot is shown, but it must be the right ones.
    let zebra = recall_text(&repo, "organon zebra", 100);
    assert!(zebra.contains("zebra rule"), "{zebra}");
    let alpha = recall_text(&repo, "organon alpha", 100);
    assert!(alpha.contains("alpha rule"), "{alpha}");
    assert!(!alpha.contains("zebra rule"), "{alpha}");
    assert_ne!(zebra, alpha, "the question changes what is shown");
}

#[test]
fn recall_budget_widens_what_a_slot_shows() {
    let (_d, repo) = project_with_decisions();
    let tight = recall_text(&repo, "organon", 1);
    assert!(
        tight.len() > 4,
        "the first item appears even past the budget"
    );
    assert_eq!(tight.matches(" rule: settled").count(), 8, "{tight}");
    let roomy = recall_text(&repo, "organon", 12000);
    assert_eq!(roomy.matches(" rule: settled").count(), 15, "{roomy}");
    assert!(!roomy.contains("more in"), "nothing cut, nothing announced");
    // A budget halfway between the two sizes shows something in between, and says so.
    let halfway = (tight.len() + roomy.len()) / 2 / 4;
    let middle = recall_text(&repo, "organon", halfway);
    let shown = middle.matches(" rule: settled").count();
    assert!((9..15).contains(&shown), "{shown} shown:\n{middle}");
    assert!(
        middle.contains(&format!("(+{} more in Decisions", 15 - shown)),
        "{middle}"
    );
    // More budget never shows fewer.
    let mut last = 0;
    for tokens in (1..400).step_by(10) {
        let n = recall_text(&repo, "organon", tokens)
            .matches(" rule: settled")
            .count();
        assert!(n >= last, "{n} < {last} at {tokens} tokens");
        last = n;
    }
}

#[test]
fn recall_stays_within_the_budget_footers_included() {
    // Regression: widening used the whole budget and the footers came on top of it.
    let (_d, repo) = project_with(12);
    // The first item always appears, so this is the least a package can be.
    let least = recall_text(&repo, "organon", 1).len();
    for tokens in (1..700).step_by(7) {
        let text = recall_text(&repo, "organon", tokens);
        assert!(
            text.len() <= (tokens * 4).max(least),
            "{} chars at {tokens} tokens (least {least}):\n{text}",
            text.len()
        );
    }
}

#[test]
fn recall_caps_each_slot_of_an_item_on_its_own() {
    let (_d, repo) = project_with(12);
    let tight = recall_text(&repo, "organon", 1);
    assert_eq!(tight.matches(" rule: settled").count(), 8, "{tight}");
    assert_eq!(tight.matches("riddle ").count(), 8, "{tight}");
    assert!(tight.contains("(+7 more in Decisions: mneme page project:organon)"));
    assert!(tight.contains("(+4 more in Open questions: mneme page project:organon)"));
    // The question picks the claim inside the slot it belongs to.
    let q = recall_text(&repo, "organon riddle 11", 1);
    assert!(q.contains("riddle 11"), "{q}");
    let roomy = recall_text(&repo, "organon", 12000);
    assert_eq!(roomy.matches("riddle ").count(), 12, "{roomy}");
    assert!(!roomy.contains("more in"));
}

#[test]
fn recall_keeps_the_frame_order_when_every_claim_ties() {
    let (_d, repo) = project_with_decisions();
    // "organon" is in no claim's text, so nothing outranks anything.
    let tight = recall_text(&repo, "organon", 1);
    for topic in [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
    ] {
        assert!(tight.contains(&format!("{topic} rule")), "{topic}: {tight}");
    }
    for topic in [
        "india", "juliet", "kilo", "lima", "mike", "november", "zebra",
    ] {
        assert!(
            !tight.contains(&format!("{topic} rule")),
            "{topic}: {tight}"
        );
    }
    // Shown in that order, too.
    assert!(tight.find("alpha rule") < tight.find("bravo rule"));
}

#[test]
fn recall_says_what_it_cut_and_where_to_find_it() {
    let (_d, repo) = project_with_decisions();
    let tight = recall_text(&repo, "organon", 1);
    assert!(
        tight.contains("(+7 more in Decisions: mneme page project:organon)"),
        "{tight}"
    );
}
