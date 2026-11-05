mod capture;
mod links;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use mneme_core::digest::{ops_from_proposal, Proposal};
use mneme_core::frames::{coverage, load_frames, profile};
use mneme_core::project::{write_all, BuildInfo};
use mneme_core::util::{now_rfc3339, sha256_hex, short_hash};
use mneme_core::*;

#[derive(Parser)]
#[command(name = "mneme", version, about = "A memory that comprehends: claim log, history, frames, views")]
struct Cli {
    /// Repository root (default: search upward from the current directory)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create memory/ in the current (or --root) directory
    Init,
    /// Capture sources into the memory
    Capture {
        #[command(subcommand)]
        what: CaptureCmd,
    },
    /// Commit a digest proposal (JSON) as claims
    Digest { file: PathBuf },
    /// Rebuild projections (graph, wiki, view.html)
    Build {
        /// Build the memory as it was at this commit or branch
        #[arg(long)]
        at: Option<String>,
        #[arg(long, default_value = "projections")]
        out: PathBuf,
    },
    /// Retract claims that were wrong (kept in history, no longer active)
    Retract {
        claims: Vec<String>,
        #[arg(long)]
        reason: String,
        #[arg(long, default_value = "philip")]
        author: String,
    },
    /// Commit history of the current branch
    Log {
        #[arg(short, long, default_value_t = 20)]
        n: usize,
    },
    /// Show a commit's ops
    Show { commit: String },
    /// What changed between two commits or branches
    Diff { from: String, to: String },
    /// List branches, or create one at HEAD (or --at)
    Branch {
        name: Option<String>,
        #[arg(long)]
        at: Option<String>,
    },
    /// Switch the current branch
    Switch { name: String },
    /// Counts and current branch
    Status,
    /// Print what the memory knows about an anchor, slot by slot
    Page { id: String },
    /// Ask the memory: compile a context package for a question (and log the episode)
    Recall {
        question: Vec<String>,
        /// Token budget for the package (~4 chars per token)
        #[arg(long, default_value_t = 4000)]
        budget: usize,
        #[arg(short, long, default_value_t = 25)]
        n: usize,
        /// One line per item (~10x cheaper); then `mneme page <id>` for detail
        #[arg(long)]
        brief: bool,
    },
    /// Say whether a recall helped: feeds the learning loop
    Feedback {
        episode: String,
        /// good | partial | bad
        verdict: String,
        /// Anchor ids that were actually used
        #[arg(long, value_delimiter = ',')]
        used: Vec<String>,
        /// What was missing
        #[arg(long)]
        missing: Option<String>,
    },
    /// Anchors with the least understanding: empty frame slots, ranked
    Frontier {
        #[arg(long)]
        kind: Option<String>,
        #[arg(short, long, default_value_t = 15)]
        n: usize,
    },
}

#[derive(Subcommand)]
enum CaptureCmd {
    /// A GitHub user's starred repositories (public API; GITHUB_TOKEN used if set)
    GithubStars {
        #[arg(long)]
        user: String,
        /// Anchor id of the person who starred them
        #[arg(long, default_value = "person:philip")]
        person: String,
    },
    /// Links Philip saved (web pages, X posts/accounts, GitHub repos, HF datasets), with notes
    Links {
        /// Text file: one URL per line, optional note after it; `# group` lines
        file: PathBuf,
        /// Do not fetch links found inside posts
        #[arg(long)]
        no_follow: bool,
        /// Who saved these links (anchor id), or `none` when research found them
        #[arg(long, default_value = "person:philip")]
        person: String,
    },
    /// The README of GitHub repos already in memory (deeper source for digest)
    Readme {
        /// owner/name, one or more
        repos: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    if let Cmd::Init = cli.cmd {
        let root = cli.root.unwrap_or(cwd);
        Repo::init(&root)?;
        println!("initialized mneme memory in {}", root.join("memory").display());
        return Ok(());
    }
    // Where the memory lives: --root, else $MNEME_ROOT, else search upward from here.
    // Keeping the memory in its own (private) repo and the engine in another is the normal setup.
    let start = cli.root.clone().or_else(|| std::env::var_os("MNEME_ROOT").map(PathBuf::from)).unwrap_or(cwd);
    let repo = Repo::discover(&start)?;

    match cli.cmd {
        Cmd::Init => unreachable!(),
        Cmd::Capture { what: CaptureCmd::GithubStars { user, person } } => {
            let (_, state) = repo.state_at(None)?;
            let ops = capture::github_stars(&repo, &state, &user, &person)?;
            let n = ops.len();
            let c = repo.commit("mneme/capture-github@0.1", &format!("capture: GitHub stars of {user}"), ops)?;
            println!("{} committed ({n} ops)", c.id);
        }
        Cmd::Capture { what: CaptureCmd::Links { file, no_follow, person } } => {
            let text = std::fs::read_to_string(&file)?;
            let entries = links::parse_file(text.trim_start_matches('\u{feff}'));
            let (_, state) = repo.state_at(None)?;
            let (ops, failures) = links::capture(&repo, &state, &entries, &person, !no_follow)?;
            let n = ops.len();
            let msg = format!("capture: {} saved links from {}", entries.len(), file.display());
            let c = repo.commit("mneme/capture-links@0.1", &msg, ops)?;
            println!("{} committed ({n} ops); {} failed", c.id, failures.len());
            for f in failures {
                println!("  failed: {f}");
            }
        }
        Cmd::Capture { what: CaptureCmd::Readme { repos } } => {
            let (_, state) = repo.state_at(None)?;
            let ops = capture::readmes(&repo, &state, &repos)?;
            let n = ops.len();
            let c = repo.commit("mneme/capture-github@0.1", &format!("capture: READMEs of {}", repos.join(", ")), ops)?;
            println!("{} committed ({n} ops)", c.id);
        }
        Cmd::Recall { question, budget, n, brief } => {
            let q = question.join(" ");
            let (lineage, state) = repo.state_at(None)?;
            let frames = load_frames(&repo.frames_dir())?;
            let head = lineage.last().map(|c| c.id.clone()).unwrap_or_default();
            let r = mneme_core::recall::recall(&state, &q, n);
            let pkg = if brief {
                mneme_core::recall::render_brief(&state, &frames, &r, &head)
            } else {
                mneme_core::recall::render(&state, &frames, &r, &head, budget)
            };
            let ep = short_hash("e_", format!("{q}{}", now_rfc3339()).as_bytes());
            repo.append_episode(&serde_json::json!({
                "type": "recall", "id": ep, "time": now_rfc3339(), "head": head, "question": q,
                "budget": budget, "included": pkg.included, "omitted": pkg.omitted,
                "scores": r.items.iter().map(|i| (i.id.clone(), (i.score * 1000.0).round() / 1000.0)).collect::<Vec<_>>()
            }))?;
            println!("{}episode: {ep}  (rate it: mneme feedback {ep} good|partial|bad --used id,id --missing \"…\")", pkg.markdown);
        }
        Cmd::Feedback { episode, verdict, used, missing } => {
            if !["good", "partial", "bad"].contains(&verdict.as_str()) {
                anyhow::bail!("verdict must be good, partial or bad");
            }
            repo.append_episode(&serde_json::json!({
                "type": "feedback", "episode": episode, "time": now_rfc3339(), "verdict": verdict,
                "used": used, "missing": missing
            }))?;
            println!("feedback recorded for {episode}");
        }
        Cmd::Digest { file } => {
            let text = std::fs::read_to_string(&file)?;
            let text = text.trim_start_matches('\u{feff}');
            let p: Proposal = serde_json::from_str(text)?;
            let (_, state) = repo.state_at(None)?;
            let hash = sha256_hex(text.as_bytes());
            let id = short_hash("s_", text.as_bytes());
            repo.save_source_payload(&id, "json", text.as_bytes())?;
            let source = Source {
                id,
                kind: "digest-proposal".into(),
                uri: file.to_string_lossy().replace('\\', "/"),
                title: p.message.clone(),
                captured_at: now_rfc3339(),
                content_hash: hash,
                meta: Default::default(),
            };
            let ops = ops_from_proposal(&state, &p, source)?;
            let n = ops.len();
            let c = repo.commit(&p.author, &p.message, ops)?;
            println!("{} committed ({n} ops): {}", c.id, p.message);
        }
        Cmd::Build { at, out } => {
            let (lineage, state) = repo.state_at(at.as_deref())?;
            let frames = load_frames(&repo.frames_dir())?;
            let refs = repo.refs()?;
            let out = if out.is_absolute() { out } else { repo.root.join(out) };
            write_all(&out, &lineage, &state, &frames, &BuildInfo { branch: &refs.head, branches: &refs.branches })?;
            println!(
                "built {} anchors, {} claims, {} commits -> {}",
                state.anchors.len(),
                state.claims.len(),
                lineage.len(),
                out.join("view.html").display()
            );
        }
        Cmd::Retract { claims, reason, author } => {
            let (_, state) = repo.state_at(None)?;
            let mut ops = vec![];
            for id in &claims {
                match state.claims.get(id) {
                    Some(cs) if cs.status == Status::Active => ops.push(Op::SetStatus {
                        claim: id.clone(), status: Status::Retracted, reason: Some(reason.clone()), replaced_by: None,
                    }),
                    Some(_) => eprintln!("{id}: already not active"),
                    None => anyhow::bail!("unknown claim {id}"),
                }
            }
            let n = ops.len();
            let c = repo.commit(&author, &format!("retract {n} claim(s): {reason}"), ops)?;
            println!("{} committed ({n} retracted)", c.id);
        }
        Cmd::Log { n } => {
            let (lineage, _) = repo.state_at(None)?;
            for (i, c) in lineage.iter().enumerate().rev().take(n) {
                println!("{i:>3} {} {}  {}  ({} ops, {})", c.id, &c.time[..16], c.message, c.ops.len(), c.author);
            }
        }
        Cmd::Show { commit } => {
            let id = repo.resolve(&commit)?;
            let c = repo.commits()?.into_iter().find(|c| c.id == id).unwrap();
            println!("commit {}\nparents {:?}\nauthor {}\ntime {}\n\n    {}\n", c.id, c.parents, c.author, c.time, c.message);
            for op in &c.ops {
                match op {
                    Op::AddSource { source } => println!("+ source {} {} {}", source.id, source.kind, source.title),
                    Op::AddAnchor { anchor } => println!("+ anchor {} \"{}\"", anchor.id, anchor.name),
                    Op::AddClaim { claim } => println!(
                        "+ claim  {} --{}--> {} [{}]",
                        claim.about,
                        claim.relation,
                        claim.value.display().chars().take(70).collect::<String>(),
                        claim.basis.as_str()
                    ),
                    Op::SetStatus { claim, status, .. } => println!("~ status {claim} -> {}", status.as_str()),
                }
            }
        }
        Cmd::Diff { from, to } => {
            let (_, a) = repo.state_at(Some(&from))?;
            let (_, b) = repo.state_at(Some(&to))?;
            for (id, an) in &b.anchors {
                if !a.anchors.contains_key(id) {
                    println!("+ anchor {id} \"{}\"", an.anchor.name);
                }
            }
            for (id, cs) in &b.claims {
                match a.claims.get(id) {
                    None => println!("+ {} --{}--> {}", cs.claim.about, cs.claim.relation, cs.claim.value.display()),
                    Some(old) if old.status != cs.status => println!(
                        "~ {} --{}--> {}: {} -> {}",
                        cs.claim.about,
                        cs.claim.relation,
                        cs.claim.value.display(),
                        old.status.as_str(),
                        cs.status.as_str()
                    ),
                    _ => {}
                }
            }
        }
        Cmd::Branch { name, at } => match name {
            Some(n) => {
                repo.create_branch(&n, at.as_deref())?;
                println!("created branch {n} (switch with `mneme switch {n}`)");
            }
            None => {
                let refs = repo.refs()?;
                for (b, id) in &refs.branches {
                    println!("{} {b} {id}", if *b == refs.head { "*" } else { " " });
                }
            }
        },
        Cmd::Switch { name } => {
            repo.switch(&name)?;
            println!("on branch {name}");
        }
        Cmd::Status => {
            let refs = repo.refs()?;
            let (lineage, state) = repo.state_at(None)?;
            println!(
                "branch {}  head {}\n{} commits · {} sources · {} anchors · {} claims ({} active)",
                refs.head,
                lineage.last().map(|c| c.id.as_str()).unwrap_or("-"),
                lineage.len(),
                state.sources.len(),
                state.anchors.len(),
                state.claims.len(),
                state.active().count()
            );
        }
        Cmd::Page { id } => {
            let (_, state) = repo.state_at(None)?;
            let frames = load_frames(&repo.frames_dir())?;
            let a = state.anchors.get(&id).ok_or_else(|| anyhow::anyhow!("unknown anchor {id}"))?;
            println!("{} ({})\n{}", a.anchor.name, id, a.anchor.summary.clone().unwrap_or_default());
            if let Some(f) = frames.iter().find(|f| f.kind == a.anchor.kind) {
                let fills = profile(f, &state, &id);
                println!("understanding: {:.0}%", coverage(&fills) * 100.0);
                for fill in fills {
                    let mark = if fill.claims.is_empty() { "[ ]" } else { "[x]" };
                    println!("{mark} {}", fill.slot.label);
                    for cs in fill.claims {
                        // Detail view: full text. Incoming links read from the other side ("← X part_of").
                        let c = &cs.claim;
                        let conf = c.confidence.map(|x| format!(" {x:.2}")).unwrap_or_default();
                        if c.about == id {
                            println!("      {} {} ({}{conf})", c.relation, c.value.display(), c.basis.as_str());
                        } else {
                            println!("      ← {} {} ({}{conf})", c.about, c.relation, c.basis.as_str());
                        }
                    }
                }
            }
        }
        Cmd::Frontier { kind, n } => {
            let (_, state) = repo.state_at(None)?;
            let frames = load_frames(&repo.frames_dir())?;
            let mut rows = vec![];
            for a in state.anchors.values() {
                if kind.as_deref().is_some_and(|k| k != a.anchor.kind) {
                    continue;
                }
                if let Some(f) = frames.iter().find(|f| f.kind == a.anchor.kind) {
                    let fills = profile(f, &state, &a.anchor.id);
                    // Connectedness through meaning, not metadata (topics/owners/stars don't count).
                    let meaningful = |cs: &&mneme_core::state::ClaimState| {
                        !matches!(mneme_core::relations::info(&cs.claim.relation).family, "meta" | "topic" | "personal")
                    };
                    let degree = state.pointing_to(&a.anchor.id).filter(meaningful).count()
                        + state.about(&a.anchor.id).filter(meaningful).count();
                    let empty: Vec<String> =
                        fills.iter().filter(|f| f.claims.is_empty()).map(|f| f.slot.label.clone()).collect();
                    // Importance: connected things we barely understand come first.
                    let score = degree as f32 * (1.0 - coverage(&fills));
                    rows.push((score, a.anchor.id.clone(), coverage(&fills), empty));
                }
            }
            rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            for (score, id, cov, empty) in rows.into_iter().take(n) {
                println!("{score:>6.1}  {id}  {:.0}% understood\n        missing: {}", cov * 100.0, empty.join(", "));
            }
        }
    }
    Ok(())
}
