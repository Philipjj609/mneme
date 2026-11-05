//! `mneme`: a memory that comprehends. See `mneme --help`.

#![forbid(unsafe_code)]

mod commands;
mod github;
mod links;
mod net;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mneme",
    version,
    about = "A memory that comprehends: claim log, history, frames, recall, views"
)]
struct Cli {
    /// Memory folder (default: $MNEME_ROOT, else search upward from the current directory)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a memory (memory/ + default frames/) in the current or --root folder
    Init,
    /// Capture sources into the memory
    Capture {
        #[command(subcommand)]
        what: Capture,
    },
    /// Commit a digest proposal (JSON) as claims
    Digest { file: PathBuf },
    /// Retract claims that were wrong (kept in history, no longer active)
    Retract {
        claims: Vec<String>,
        #[arg(long)]
        reason: String,
        #[arg(long, default_value = "philip")]
        author: String,
    },
    /// Rebuild projections (graph.json, view.html, wiki)
    Build {
        /// Build the memory as it was at this commit or branch
        #[arg(long)]
        at: Option<String>,
        #[arg(long, default_value = "projections")]
        out: PathBuf,
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
    /// Ask the memory: a context package for a question (logged as an episode)
    Recall {
        question: Vec<String>,
        /// Token budget for the package (~4 characters per token)
        #[arg(long, default_value_t = 4000)]
        budget: usize,
        #[arg(short, long, default_value_t = 25)]
        n: usize,
        /// One line per item (~10x cheaper); then `mneme page <id>` for detail
        #[arg(long)]
        brief: bool,
    },
    /// Rate a recall: the learning signal
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
    /// Everything known about one thing, slot by slot
    Page { id: String },
    /// The most connected, least understood things: what to research next
    Frontier {
        #[arg(long)]
        kind: Option<String>,
        #[arg(short, long, default_value_t = 15)]
        n: usize,
    },
}

#[derive(Subcommand)]
enum Capture {
    /// A GitHub user's starred repositories (GITHUB_TOKEN used if set)
    GithubStars {
        #[arg(long)]
        user: String,
        /// Anchor id of the person who starred them
        #[arg(long, default_value = "person:philip")]
        person: String,
    },
    /// Links (web pages, X posts/accounts, GitHub repos, HF datasets) with notes
    Links {
        /// One URL per line, optional note after it; `# group` lines
        file: PathBuf,
        /// Do not fetch links found inside posts
        #[arg(long)]
        no_follow: bool,
        /// Who saved these links (anchor id), or `none` when research found them
        #[arg(long, default_value = "person:philip")]
        person: String,
    },
    /// READMEs of GitHub repos already in memory
    Readme { repos: Vec<String> },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Cmd::Init = cli.cmd {
        let root = match cli.root {
            Some(r) => r,
            None => std::env::current_dir()?,
        };
        return commands::init(&root);
    }
    let repo = commands::locate(cli.root)?;
    match cli.cmd {
        Cmd::Init => unreachable!("handled above"),
        Cmd::Capture {
            what: Capture::GithubStars { user, person },
        } => commands::capture_stars(&repo, &user, &person),
        Cmd::Capture {
            what:
                Capture::Links {
                    file,
                    no_follow,
                    person,
                },
        } => commands::capture_links(&repo, &file, &person, !no_follow),
        Cmd::Capture {
            what: Capture::Readme { repos },
        } => commands::capture_readmes(&repo, &repos),
        Cmd::Digest { file } => commands::digest(&repo, &file),
        Cmd::Retract {
            claims,
            reason,
            author,
        } => commands::retract(&repo, &claims, &reason, &author),
        Cmd::Build { at, out } => commands::build(&repo, at.as_deref(), &out),
        Cmd::Log { n } => commands::log(&repo, n),
        Cmd::Show { commit } => commands::show(&repo, &commit),
        Cmd::Diff { from, to } => commands::diff(&repo, &from, &to),
        Cmd::Branch { name, at } => commands::branch(&repo, name.as_deref(), at.as_deref()),
        Cmd::Switch { name } => commands::switch(&repo, &name),
        Cmd::Status => commands::status(&repo),
        Cmd::Recall {
            question,
            budget,
            n,
            brief,
        } => commands::recall_cmd(&repo, &question.join(" "), budget, n, brief),
        Cmd::Feedback {
            episode,
            verdict,
            used,
            missing,
        } => commands::feedback(&repo, &episode, &verdict, &used, missing.as_deref()),
        Cmd::Page { id } => commands::page(&repo, &id),
        Cmd::Frontier { kind, n } => commands::frontier(&repo, kind.as_deref(), n),
    }
}
