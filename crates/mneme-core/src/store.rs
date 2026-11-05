//! On-disk layout of a memory folder (plain text, git-friendly):
//!
//! ```text
//! memory/log.jsonl       one commit per line: the authoritative memory (append-only)
//! memory/refs.json       branches and HEAD
//! memory/sources/<id>.*  captured evidence (local; may hold third-party content)
//! memory/episodes.jsonl  recall and feedback log
//! frames/*.json          comprehension frames
//! ```

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::{Commit, CommitId, Op, Refs};
use crate::state::State;

/// Frames written by `init`, embedded from the repo's `frames/` at build time.
pub const DEFAULT_FRAMES: &[(&str, &str)] = &[
    ("account.json", include_str!("../../../frames/account.json")),
    ("concept.json", include_str!("../../../frames/concept.json")),
    ("dataset.json", include_str!("../../../frames/dataset.json")),
    ("post.json", include_str!("../../../frames/post.json")),
    ("project.json", include_str!("../../../frames/project.json")),
    ("repo.json", include_str!("../../../frames/repo.json")),
    ("site.json", include_str!("../../../frames/site.json")),
];

/// A memory folder.
#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Create (or complete) a memory in `root`: `memory/`, empty log, refs, default frames.
    ///
    /// # Errors
    /// I/O failures creating the folders and files.
    pub fn init(root: &Path) -> Result<Self> {
        let repo = Self {
            root: root.to_path_buf(),
        };
        let sources = repo.memory_dir().join("sources");
        fs::create_dir_all(&sources).map_err(Error::io(&sources))?;
        if !repo.refs_path().exists() {
            repo.save_refs(&Refs::default())?;
        }
        if !repo.log_path().exists() {
            write(&repo.log_path(), b"")?;
        }
        let frames = repo.frames_dir();
        if !frames.exists() {
            fs::create_dir_all(&frames).map_err(Error::io(&frames))?;
            for (name, body) in DEFAULT_FRAMES {
                write(&frames.join(name), body.as_bytes())?;
            }
        }
        Ok(repo)
    }

    /// Open the memory at or above `start` (the first folder containing `memory/refs.json`).
    ///
    /// # Errors
    /// [`Error::NoMemory`] if none is found.
    pub fn discover(start: &Path) -> Result<Self> {
        start
            .ancestors()
            .find(|d| d.join("memory").join("refs.json").exists())
            .map(|d| Self {
                root: d.to_path_buf(),
            })
            .ok_or_else(|| Error::NoMemory(start.to_path_buf()))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
    #[must_use]
    pub fn memory_dir(&self) -> PathBuf {
        self.root.join("memory")
    }
    #[must_use]
    pub fn frames_dir(&self) -> PathBuf {
        self.root.join("frames")
    }
    fn refs_path(&self) -> PathBuf {
        self.memory_dir().join("refs.json")
    }
    fn log_path(&self) -> PathBuf {
        self.memory_dir().join("log.jsonl")
    }

    /// # Errors
    /// Missing or malformed `refs.json`.
    pub fn refs(&self) -> Result<Refs> {
        let path = self.refs_path();
        let text = read(&path)?;
        serde_json::from_str(strip_bom(&text)).map_err(|source| Error::Json { path, source })
    }

    /// # Errors
    /// I/O failure writing `refs.json`.
    pub fn save_refs(&self, refs: &Refs) -> Result<()> {
        let path = self.refs_path();
        let text = serde_json::to_string_pretty(refs).map_err(|source| Error::Json {
            path: path.clone(),
            source,
        })?;
        write(&path, format!("{text}\n").as_bytes())
    }

    /// Every commit on every branch, in the order written.
    ///
    /// # Errors
    /// I/O failure or a malformed line (reported with its line number).
    pub fn commits(&self) -> Result<Vec<Commit>> {
        let path = self.log_path();
        let text = read(&path)?;
        text.lines()
            .enumerate()
            .map(|(i, l)| (i, strip_bom(l)))
            .filter(|(_, l)| !l.trim().is_empty())
            .map(|(i, l)| {
                serde_json::from_str(l).map_err(|source| Error::Parse {
                    path: path.clone(),
                    line: i + 1,
                    source,
                })
            })
            .collect()
    }

    /// # Errors
    /// See [`Repo::refs`].
    pub fn head(&self) -> Result<Option<CommitId>> {
        let refs = self.refs()?;
        Ok(refs.branches.get(&refs.head).cloned())
    }

    /// Resolve `HEAD`, a branch name, or a unique prefix of a commit id.
    ///
    /// # Errors
    /// Unknown or ambiguous revision, or an empty memory for `HEAD`.
    pub fn resolve(&self, spec: &str) -> Result<CommitId> {
        let refs = self.refs()?;
        if let Some(id) = refs.branches.get(spec) {
            return Ok(id.clone());
        }
        if spec == "HEAD" {
            return refs.branches.get(&refs.head).cloned().ok_or(Error::Empty);
        }
        let matches: Vec<CommitId> = self
            .commits()?
            .into_iter()
            .map(|c| c.id)
            .filter(|id| id.as_str().starts_with(spec))
            .collect();
        match matches.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err(Error::UnknownRevision(spec.to_owned())),
            many => Err(Error::AmbiguousRevision {
                spec: spec.to_owned(),
                count: many.len(),
            }),
        }
    }

    /// Commits from the root to `tip`, following first parents.
    ///
    /// # Errors
    /// A parent that is not in the log.
    pub fn lineage(&self, tip: &CommitId) -> Result<Vec<Commit>> {
        let mut all: HashMap<CommitId, Commit> = self
            .commits()?
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();
        let mut out = Vec::new();
        let mut cur = Some(tip.clone());
        while let Some(id) = cur {
            let commit = all
                .remove(&id)
                .ok_or_else(|| Error::MissingCommit(id.to_string()))?;
            cur = commit.parents.first().cloned();
            out.push(commit);
        }
        out.reverse();
        Ok(out)
    }

    /// Lineage and replayed state at `at` (HEAD when `None`). An empty memory gives an empty state.
    ///
    /// # Errors
    /// See [`Repo::resolve`] and [`Repo::lineage`].
    pub fn state_at(&self, at: Option<&str>) -> Result<(Vec<Commit>, State)> {
        let tip = match at {
            Some(spec) => Some(self.resolve(spec)?),
            None => self.head()?,
        };
        match tip {
            None => Ok((Vec::new(), State::default())),
            Some(tip) => {
                let lineage = self.lineage(&tip)?;
                let state = State::replay(&lineage);
                Ok((lineage, state))
            }
        }
    }

    /// Append a commit of `ops` on the current branch.
    ///
    /// # Errors
    /// [`Error::NothingToCommit`] for an empty batch; I/O failures.
    pub fn commit(&self, author: &str, message: &str, ops: Vec<Op>) -> Result<Commit> {
        if ops.is_empty() {
            return Err(Error::NothingToCommit);
        }
        let mut refs = self.refs()?;
        let parents: Vec<CommitId> = refs.branches.get(&refs.head).cloned().into_iter().collect();
        let commit = Commit::new(parents, author, message, crate::util::now_rfc3339(), ops);
        let path = self.log_path();
        let line = serde_json::to_string(&commit).map_err(|source| Error::Json {
            path: path.clone(),
            source,
        })?;
        let mut f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(Error::io(&path))?;
        writeln!(f, "{line}").map_err(Error::io(&path))?;
        refs.branches.insert(refs.head.clone(), commit.id.clone());
        self.save_refs(&refs)?;
        Ok(commit)
    }

    /// # Errors
    /// The branch exists, the revision is unknown, or the memory is empty.
    pub fn create_branch(&self, name: &str, at: Option<&str>) -> Result<()> {
        let mut refs = self.refs()?;
        if refs.branches.contains_key(name) {
            return Err(Error::BranchExists(name.to_owned()));
        }
        let at = match at {
            Some(spec) => self.resolve(spec)?,
            None => refs.branches.get(&refs.head).cloned().ok_or(Error::Empty)?,
        };
        refs.branches.insert(name.to_owned(), at);
        self.save_refs(&refs)
    }

    /// # Errors
    /// No such branch.
    pub fn switch(&self, name: &str) -> Result<()> {
        let mut refs = self.refs()?;
        if !refs.branches.contains_key(name) {
            return Err(Error::NoSuchBranch(name.to_owned()));
        }
        name.clone_into(&mut refs.head);
        self.save_refs(&refs)
    }

    /// Store a captured payload as `memory/sources/<id>.<ext>` (once).
    ///
    /// # Errors
    /// I/O failure.
    pub fn save_source_payload(&self, id: &str, ext: &str, content: &[u8]) -> Result<PathBuf> {
        let path = self
            .memory_dir()
            .join("sources")
            .join(format!("{id}.{ext}"));
        if !path.exists() {
            write(&path, content)?;
        }
        Ok(path)
    }

    /// Append to the usage log (recall + feedback): the learning signal.
    ///
    /// # Errors
    /// I/O failure.
    pub fn append_episode(&self, record: &serde_json::Value) -> Result<()> {
        let path = self.memory_dir().join("episodes.jsonl");
        let mut f = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(Error::io(&path))?;
        writeln!(f, "{record}").map_err(Error::io(&path))
    }
}

fn strip_bom(s: &str) -> &str {
    s.trim_start_matches('\u{feff}')
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(Error::io(path))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).map_err(Error::io(path))
}
