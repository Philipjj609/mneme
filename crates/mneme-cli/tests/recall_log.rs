//! `mneme recall` logging: an episode is written by default and not with `--no-log`.
//! Runs the real binary against a temporary memory folder.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};

/// A small proposal so recalls have real items to brief (not just "nothing matches").
const PROPOSAL: &str = r#"{
  "author": "test", "message": "seed",
  "anchors": [
    {"id": "concept:motion-design", "name": "Motion design", "summary": "Animation rendered from code"},
    {"id": "tool:hyperframes", "name": "Hyperframes", "summary": "HTML to video renderer"}
  ],
  "claims": [
    ["tool:hyperframes", "provides", "concept:motion-design"],
    ["concept:motion-design", "defined_as", "animation rendered from code"]
  ]
}"#;

fn mneme(root: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_mneme"))
        .arg("--root")
        .arg(root)
        .args(args)
        .env_remove("MNEME_ROOT")
        .output()
        .expect("run mneme");
    assert!(
        out.status.success(),
        "mneme {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// An initialised memory seeded through `mneme digest`.
fn fresh() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    mneme(dir.path(), &["init"]);
    let proposal = tempfile::tempdir().unwrap();
    let file = proposal.path().join("proposal.json");
    std::fs::write(&file, PROPOSAL).unwrap();
    mneme(dir.path(), &["digest", file.to_str().unwrap()]);
    dir
}

/// Every file under the memory root: relative path (with `/`) -> contents.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).unwrap();
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn episodes(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join("memory").join("episodes.jsonl")).ok()
}

#[test]
fn no_log_writes_no_episode_for_normal_and_brief() {
    let dir = fresh();
    let root = dir.path();
    let before = snapshot(root);
    assert!(!before.is_empty());
    for args in [
        &["recall", "secret question one", "--no-log"][..],
        &["recall", "secret question two", "--brief", "--no-log"][..],
    ] {
        let out = stdout(&mneme(root, args));
        assert!(!out.contains("episode:"), "no episode line: {out}");
    }
    assert_eq!(episodes(root), None, "episodes.jsonl must not be created");
    assert_eq!(snapshot(root), before, "memory root must be unchanged");
}

#[test]
fn no_log_leaves_an_existing_log_untouched() {
    let dir = fresh();
    let root = dir.path();
    mneme(root, &["recall", "logged first"]);
    let before = episodes(root).expect("default recall creates the log");
    let tree = snapshot(root);
    mneme(root, &["recall", "not logged", "--no-log"]);
    mneme(
        root,
        &["recall", "not logged either", "--brief", "--no-log"],
    );
    let after = episodes(root).unwrap();
    assert_eq!(after, before);
    assert!(!after.contains("not logged"));
    assert_eq!(snapshot(root), tree, "memory root must be unchanged");
}

#[test]
fn no_log_prints_the_same_briefing_as_a_logged_recall() {
    let dir = fresh();
    let root = dir.path();
    for brief in [false, true] {
        let mut logged = vec!["recall", "motion design"];
        let mut quiet = vec!["recall", "motion design", "--no-log"];
        if brief {
            logged.push("--brief");
            quiet.push("--brief");
        }
        let logged = stdout(&mneme(root, &logged));
        let quiet = stdout(&mneme(root, &quiet));
        // The briefing has real items, so this is not two "nothing matches" outputs.
        assert!(
            quiet.contains("Motion design") || quiet.contains("concept:motion-design"),
            "seeded item in the briefing (brief={brief}): {quiet}"
        );
        let (body, tail) = logged.rsplit_once("episode: ").expect("episode line");
        assert!(tail.contains("mneme feedback"));
        assert_eq!(
            quiet, body,
            "briefing identical apart from the episode line"
        );
    }
}

#[test]
fn default_recall_still_logs_an_episode() {
    for brief in [false, true] {
        let dir = fresh();
        let root = dir.path();
        let mut args = vec!["recall", "what is logged"];
        if brief {
            args.push("--brief");
        }
        let out = stdout(&mneme(root, &args));
        assert!(out.contains("episode: e_"), "episode line printed: {out}");
        let log = episodes(root).expect("episodes.jsonl written");
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 1);
        let ep: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(ep["type"], "recall");
        assert_eq!(ep["question"], "what is logged");
    }
}

#[test]
fn json_is_unchanged_and_never_logs() {
    let dir = fresh();
    let root = dir.path();
    for extra in [&[][..], &["--no-log"][..]] {
        let mut args = vec!["recall", "ranking only", "--json"];
        args.extend_from_slice(extra);
        let v: serde_json::Value =
            serde_json::from_str(stdout(&mneme(root, &args)).trim()).unwrap();
        assert_eq!(v["query"], "ranking only");
    }
    assert_eq!(episodes(root), None);
}

#[test]
fn hook_calls_take_a_dash_leading_question_as_text_and_change_nothing() {
    let dir = fresh();
    let root = dir.path();
    let before = snapshot(root);

    // The exact shapes the hooks use.
    let brief = stdout(&mneme(
        root,
        &[
            "recall",
            "--no-log",
            "--brief",
            "-n",
            "8",
            "--",
            "-motion design",
        ],
    ));
    assert!(brief.contains("recall: -motion design"), "{brief}");
    assert!(!brief.contains("episode:"), "{brief}");

    let json = stdout(&mneme(
        root,
        &[
            "recall",
            "--json",
            "-n",
            "8",
            "--no-log",
            "--",
            "-motion design",
        ],
    ));
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(v["query"], "-motion design");

    assert_eq!(episodes(root), None);
    assert_eq!(snapshot(root), before, "memory root must be unchanged");
}
