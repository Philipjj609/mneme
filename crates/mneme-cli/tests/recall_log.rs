//! `mneme recall` logging: an episode is written by default and not with `--no-log`.
//! Runs the real binary against a temporary memory folder.

use std::path::Path;
use std::process::{Command, Output};

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

fn fresh() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    mneme(dir.path(), &["init"]);
    dir
}

fn episodes(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join("memory").join("episodes.jsonl")).ok()
}

#[test]
fn no_log_writes_no_episode_for_normal_and_brief() {
    let dir = fresh();
    let root = dir.path();
    let before = episodes(root);
    for args in [
        &["recall", "secret question one", "--no-log"][..],
        &["recall", "secret question two", "--brief", "--no-log"][..],
    ] {
        let out = stdout(&mneme(root, args));
        assert!(!out.contains("episode:"), "no episode line: {out}");
    }
    assert_eq!(episodes(root), before, "episodes.jsonl must be unchanged");
}

#[test]
fn no_log_leaves_an_existing_log_untouched() {
    let dir = fresh();
    let root = dir.path();
    mneme(root, &["recall", "logged first"]);
    let before = episodes(root).expect("default recall creates the log");
    mneme(root, &["recall", "not logged", "--no-log"]);
    mneme(
        root,
        &["recall", "not logged either", "--brief", "--no-log"],
    );
    let after = episodes(root).unwrap();
    assert_eq!(after, before);
    assert!(!after.contains("not logged"));
}

#[test]
fn no_log_prints_the_same_briefing_as_a_logged_recall() {
    let dir = fresh();
    let root = dir.path();
    for brief in [false, true] {
        let mut logged = vec!["recall", "same question"];
        let mut quiet = vec!["recall", "same question", "--no-log"];
        if brief {
            logged.push("--brief");
            quiet.push("--brief");
        }
        let logged = stdout(&mneme(root, &logged));
        let quiet = stdout(&mneme(root, &quiet));
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
