//! Command implementations: read args, call the core, render the result. The core returns
//! data; only this layer prints.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use mneme_core::frames::{coverage, profile};
use mneme_core::model::{ClaimId, Op, Source, SourceId, Status};
use mneme_core::project::{write_all, BuildInfo};
use mneme_core::recall::{recall, render, render_brief};
use mneme_core::util::now_rfc3339;
use mneme_core::{Frames, Repo};
use organon_protocol::{sha256_hex, Proposal};

use crate::links;
use crate::net::Http;

fn stderr_log(line: &str) {
    eprintln!("{line}");
}

fn commit(repo: &Repo, author: &str, message: &str, ops: Vec<Op>) -> anyhow::Result<()> {
    let n = ops.len();
    let c = repo.commit(author, message, ops)?;
    println!("{} committed ({n} ops): {message}", c.id);
    Ok(())
}

// ---- memory -------------------------------------------------------------------------------

pub fn init(root: &Path) -> anyhow::Result<()> {
    Repo::init(root)?;
    println!(
        "initialized mneme memory in {}",
        root.join("memory").display()
    );
    Ok(())
}

pub fn digest(repo: &Repo, file: &Path) -> anyhow::Result<()> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let text = text.trim_start_matches('\u{feff}');
    let proposal: Proposal = serde_json::from_str(text)
        .with_context(|| format!("parsing proposal {}", file.display()))?;
    let (_, state) = repo.state_at(None)?;
    let id = SourceId::of(text.as_bytes());
    repo.save_source_payload(id.as_str(), "json", text.as_bytes())?;
    let archive = Source {
        id,
        kind: "digest-proposal".into(),
        uri: file.to_string_lossy().replace('\\', "/"),
        title: proposal.message.clone(),
        captured_at: now_rfc3339(),
        content_hash: sha256_hex(text.as_bytes()),
        files: Default::default(),
        meta: Default::default(),
        produced_by: None,
    };
    let ops = mneme_core::digest::ops_from_proposal(&state, &proposal, archive)?;
    commit(repo, &proposal.author, &proposal.message, ops)
}

pub fn retract(repo: &Repo, claims: &[String], reason: &str, author: &str) -> anyhow::Result<()> {
    let (_, state) = repo.state_at(None)?;
    let mut ops = Vec::new();
    for raw in claims {
        let id: ClaimId = raw.parse()?;
        match state.claims.get(&id) {
            Some(cs) if cs.is_active() => {
                ops.push(Op::SetStatus {
                    claim: id,
                    status: Status::Retracted,
                    reason: Some(reason.to_owned()),
                    replaced_by: None,
                });
            }
            Some(_) => eprintln!("{id}: already not active"),
            None => anyhow::bail!("unknown claim {id}"),
        }
    }
    let n = ops.len();
    commit(
        repo,
        author,
        &format!("retract {n} claim(s): {reason}"),
        ops,
    )
}

pub fn build(repo: &Repo, at: Option<&str>, out: &Path) -> anyhow::Result<()> {
    let (lineage, state) = repo.state_at(at)?;
    let frames = Frames::load(&repo.frames_dir())?;
    let refs = repo.refs()?;
    let out = if out.is_absolute() {
        out.to_path_buf()
    } else {
        repo.root().join(out)
    };
    write_all(
        &out,
        &lineage,
        &state,
        &frames,
        BuildInfo {
            branch: &refs.head,
            branches: &refs.branches,
        },
    )?;
    println!(
        "built {} anchors, {} claims, {} commits -> {}",
        state.anchors.len(),
        state.claims.len(),
        lineage.len(),
        out.join("view.html").display()
    );
    Ok(())
}

// ---- capture ------------------------------------------------------------------------------

pub fn capture_stars(repo: &Repo, user: &str, person: &str) -> anyhow::Result<()> {
    let (_, state) = repo.state_at(None)?;
    let ops = crate::github::stars(repo, &state, &Http::new(), user, person, &mut stderr_log)?;
    commit(
        repo,
        "mneme/capture-github@0.2",
        &format!("capture: GitHub stars of {user}"),
        ops,
    )
}

pub fn capture_readmes(repo: &Repo, names: &[String]) -> anyhow::Result<()> {
    let (_, state) = repo.state_at(None)?;
    let ops = crate::github::readmes(repo, &state, &Http::new(), names, &mut stderr_log)?;
    commit(
        repo,
        "mneme/capture-github@0.2",
        &format!("capture: READMEs of {}", names.join(", ")),
        ops,
    )
}

pub fn capture_links(repo: &Repo, file: &Path, person: &str, follow: bool) -> anyhow::Result<()> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let entries = links::parse_file(text.trim_start_matches('\u{feff}'));
    let (_, state) = repo.state_at(None)?;
    let opts = links::Options {
        person: (person != "none").then_some(person),
        follow,
    };
    let outcome = links::capture(repo, &state, &Http::new(), &entries, &opts, &mut stderr_log)?;
    let message = format!(
        "capture: {} saved links from {}",
        entries.len(),
        file.display()
    );
    commit(repo, "mneme/capture-links@0.2", &message, outcome.ops)?;
    println!("{} failed", outcome.failures.len());
    for f in outcome.failures {
        println!("  failed: {f}");
    }
    Ok(())
}

// ---- history ------------------------------------------------------------------------------

pub fn log(repo: &Repo, n: usize) -> anyhow::Result<()> {
    let (lineage, _) = repo.state_at(None)?;
    for (i, c) in lineage.iter().enumerate().rev().take(n) {
        println!(
            "{i:>3} {} {}  {}  ({} ops, {})",
            c.id,
            c.time.get(..16).unwrap_or(&c.time),
            c.message,
            c.ops.len(),
            c.author
        );
    }
    Ok(())
}

pub fn show(repo: &Repo, spec: &str) -> anyhow::Result<()> {
    let id = repo.resolve(spec)?;
    let c = repo
        .commits()?
        .into_iter()
        .find(|c| c.id == id)
        .context("commit vanished from log")?;
    println!(
        "commit {}\nparents {:?}\nauthor {}\ntime {}\n\n    {}\n",
        c.id, c.parents, c.author, c.time, c.message
    );
    for op in &c.ops {
        match op {
            Op::AddSource { source } => {
                println!("+ source {} {} {}", source.id, source.kind, source.title)
            }
            Op::AddAnchor { anchor } => println!("+ anchor {} \"{}\"", anchor.id, anchor.name),
            Op::AddClaim { claim } => {
                let value: String = claim.value.to_string().chars().take(70).collect();
                println!(
                    "+ claim  {} --{}--> {value} [{}]",
                    claim.about,
                    claim.relation,
                    claim.basis.as_str()
                );
            }
            Op::SetStatus { claim, status, .. } => {
                println!("~ status {claim} -> {}", status.as_str())
            }
        }
    }
    Ok(())
}

pub fn diff(repo: &Repo, from: &str, to: &str) -> anyhow::Result<()> {
    let (_, a) = repo.state_at(Some(from))?;
    let (_, b) = repo.state_at(Some(to))?;
    for (id, anchor) in &b.anchors {
        if !a.anchors.contains_key(id) {
            println!("+ anchor {id} \"{}\"", anchor.anchor.name);
        }
    }
    for (id, cs) in &b.claims {
        let c = &cs.claim;
        match a.claims.get(id) {
            None => println!("+ {} --{}--> {}", c.about, c.relation, c.value),
            Some(old) if old.status != cs.status => {
                println!(
                    "~ {} --{}--> {}: {} -> {}",
                    c.about,
                    c.relation,
                    c.value,
                    old.status.as_str(),
                    cs.status.as_str()
                );
            }
            Some(_) => {}
        }
    }
    Ok(())
}

pub fn branch(repo: &Repo, name: Option<&str>, at: Option<&str>) -> anyhow::Result<()> {
    match name {
        Some(n) => {
            repo.create_branch(n, at)?;
            println!("created branch {n} (switch with `mneme switch {n}`)");
        }
        None => {
            let refs = repo.refs()?;
            for (b, id) in &refs.branches {
                println!("{} {b} {id}", if *b == refs.head { "*" } else { " " });
            }
        }
    }
    Ok(())
}

pub fn switch(repo: &Repo, name: &str) -> anyhow::Result<()> {
    repo.switch(name)?;
    println!("on branch {name}");
    Ok(())
}

pub fn status(repo: &Repo) -> anyhow::Result<()> {
    let refs = repo.refs()?;
    let (lineage, state) = repo.state_at(None)?;
    let head = lineage.last().map_or("-".to_owned(), |c| c.id.to_string());
    println!(
        "branch {}  head {head}\n{} commits · {} sources · {} anchors · {} claims ({} active)",
        refs.head,
        lineage.len(),
        state.sources.len(),
        state.anchors.len(),
        state.claims.len(),
        state.active().count()
    );
    Ok(())
}

// ---- asking -------------------------------------------------------------------------------

pub fn recall_cmd(
    repo: &Repo,
    question: &str,
    budget: usize,
    n: usize,
    brief: bool,
) -> anyhow::Result<()> {
    let (lineage, state) = repo.state_at(None)?;
    let frames = Frames::load(&repo.frames_dir())?;
    let head = lineage.last().map(|c| c.id.to_string()).unwrap_or_default();
    let result = recall(&state, question, n);
    let package = if brief {
        render_brief(&state, &frames, &result, &head)
    } else {
        render(&state, &frames, &result, &head, budget)
    };
    let episode =
        organon_protocol::short_hash("e_", format!("{question}{}", now_rfc3339()).as_bytes());
    let scores: Vec<(String, f32)> = result
        .items
        .iter()
        .map(|i| (i.id.to_string(), (i.score * 1000.0).round() / 1000.0))
        .collect();
    repo.append_episode(&serde_json::json!({
        "type": "recall", "id": episode, "time": now_rfc3339(), "head": head, "question": question, "budget": budget,
        "included": package.included, "omitted": package.omitted, "scores": scores,
    }))?;
    println!("{}episode: {episode}  (rate it: mneme feedback {episode} good|partial|bad --used id,id --missing \"…\")", package.markdown);
    Ok(())
}

pub fn feedback(
    repo: &Repo,
    episode: &str,
    verdict: &str,
    used: &[String],
    missing: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        ["good", "partial", "bad"].contains(&verdict),
        "verdict must be good, partial or bad"
    );
    repo.append_episode(&serde_json::json!({
        "type": "feedback", "episode": episode, "time": now_rfc3339(), "verdict": verdict, "used": used, "missing": missing,
    }))?;
    println!("feedback recorded for {episode}");
    Ok(())
}

pub fn page(repo: &Repo, id: &str) -> anyhow::Result<()> {
    let (_, state) = repo.state_at(None)?;
    let frames = Frames::load(&repo.frames_dir())?;
    let a = state
        .anchor(id)
        .with_context(|| format!("unknown anchor {id}"))?;
    println!(
        "{} ({id})\n{}",
        a.anchor.name,
        a.anchor.summary.clone().unwrap_or_default()
    );
    let Some(frame) = frames.for_kind(a.anchor.kind()) else {
        return Ok(());
    };
    let fills = profile(frame, &state, id);
    println!("understanding: {:.0}%", coverage(&fills) * 100.0);
    for fill in fills {
        println!(
            "{} {}",
            if fill.is_empty() { "[ ]" } else { "[x]" },
            fill.slot.label
        );
        for cs in fill.claims {
            // Detail view: full text. Incoming links read from the other side ("← X part_of").
            let c = &cs.claim;
            let conf = c.confidence.map(|x| format!(" {x:.2}")).unwrap_or_default();
            if c.about.as_str() == id {
                println!(
                    "      {} {} ({}{conf})",
                    c.relation,
                    c.value,
                    c.basis.as_str()
                );
            } else {
                println!(
                    "      ← {} {} ({}{conf})",
                    c.about,
                    c.relation,
                    c.basis.as_str()
                );
            }
        }
    }
    Ok(())
}

pub fn frontier(repo: &Repo, kind: Option<&str>, n: usize) -> anyhow::Result<()> {
    let (_, state) = repo.state_at(None)?;
    let frames = Frames::load(&repo.frames_dir())?;
    let mut rows: Vec<(f32, String, f32, Vec<String>)> = Vec::new();
    for a in state
        .anchors
        .values()
        .filter(|a| kind.is_none_or(|k| k == a.anchor.kind()))
    {
        let Some(frame) = frames.for_kind(a.anchor.kind()) else {
            continue;
        };
        let id = a.anchor.id.as_str();
        let fills = profile(frame, &state, id);
        // Connectedness through meaning, not metadata (topics, owners, stars don't count).
        let meaningful =
            |cs: &&mneme_core::state::ClaimState| cs.claim.relation.info().family.is_meaningful();
        #[allow(clippy::cast_precision_loss)]
        let degree = (state.pointing_to(id).filter(meaningful).count()
            + state.about(id).filter(meaningful).count()) as f32;
        let cov = coverage(&fills);
        let empty = fills
            .iter()
            .filter(|f| f.is_empty())
            .map(|f| f.slot.label.clone())
            .collect();
        rows.push((degree * (1.0 - cov), id.to_owned(), cov, empty));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (score, id, cov, empty) in rows.into_iter().take(n) {
        println!(
            "{score:>6.1}  {id}  {:.0}% understood\n        missing: {}",
            cov * 100.0,
            empty.join(", ")
        );
    }
    Ok(())
}

/// Where the memory lives: `--root`, else `$MNEME_ROOT`, else search upward from here.
pub fn locate(root: Option<PathBuf>) -> anyhow::Result<Repo> {
    let start = match root.or_else(|| std::env::var_os("MNEME_ROOT").map(PathBuf::from)) {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    Ok(Repo::discover(&start)?)
}
