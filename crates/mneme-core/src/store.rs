//! On-disk layout (all plain text, git-friendly):
//!
//! memory/log.jsonl      one commit per line — the authoritative memory
//! memory/refs.json      branches and HEAD
//! memory/sources/<id>   raw captured payloads (immutable evidence)

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::model::*;
use crate::state::State;
use crate::util::now_rfc3339;

pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    pub fn init(root: &Path) -> Result<Repo> {
        let repo = Repo { root: root.to_path_buf() };
        fs::create_dir_all(repo.mem().join("sources"))?;
        if !repo.refs_path().exists() {
            repo.save_refs(&Refs::default())?;
        }
        if !repo.log_path().exists() {
            fs::write(repo.log_path(), "")?;
        }
        Ok(repo)
    }

    /// Find the repo by walking up from `start` until `memory/refs.json` exists.
    pub fn discover(start: &Path) -> Result<Repo> {
        let mut dir = Some(start);
        while let Some(d) = dir {
            if d.join("memory").join("refs.json").exists() {
                return Ok(Repo { root: d.to_path_buf() });
            }
            dir = d.parent();
        }
        bail!("no mneme memory found from {} upward (run `mneme init`)", start.display())
    }

    pub fn mem(&self) -> PathBuf {
        self.root.join("memory")
    }
    fn refs_path(&self) -> PathBuf {
        self.mem().join("refs.json")
    }
    fn log_path(&self) -> PathBuf {
        self.mem().join("log.jsonl")
    }
    pub fn frames_dir(&self) -> PathBuf {
        self.root.join("frames")
    }

    pub fn refs(&self) -> Result<Refs> {
        let text = fs::read_to_string(self.refs_path()).context("reading refs.json")?;
        Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
    }

    pub fn save_refs(&self, refs: &Refs) -> Result<()> {
        fs::write(self.refs_path(), serde_json::to_string_pretty(refs)? + "\n")?;
        Ok(())
    }

    /// Every commit on every branch, in the order written.
    pub fn commits(&self) -> Result<Vec<Commit>> {
        let text = fs::read_to_string(self.log_path()).context("reading log.jsonl")?;
        text.lines()
            .map(|l| l.trim_start_matches('\u{feff}'))
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, l)| serde_json::from_str(l).with_context(|| format!("log.jsonl line {}", i + 1)))
            .collect()
    }

    pub fn head(&self) -> Result<Option<Id>> {
        let refs = self.refs()?;
        Ok(refs.branches.get(&refs.head).cloned())
    }

    /// Resolve a branch name or a (prefix of a) commit id.
    pub fn resolve(&self, spec: &str) -> Result<Id> {
        let refs = self.refs()?;
        if let Some(id) = refs.branches.get(spec) {
            return Ok(id.clone());
        }
        if spec == "HEAD" {
            return self.head()?.ok_or_else(|| anyhow!("HEAD has no commits yet"));
        }
        let matches: Vec<Id> =
            self.commits()?.into_iter().map(|c| c.id).filter(|id| id.starts_with(spec)).collect();
        match matches.len() {
            1 => Ok(matches[0].clone()),
            0 => bail!("no branch or commit matches '{spec}'"),
            _ => bail!("'{spec}' is ambiguous ({} commits)", matches.len()),
        }
    }

    /// Commits from the root to `tip`, following first parents.
    pub fn lineage(&self, tip: &str) -> Result<Vec<Commit>> {
        let all: HashMap<Id, Commit> = self.commits()?.into_iter().map(|c| (c.id.clone(), c)).collect();
        let mut out = vec![];
        let mut cur = Some(tip.to_string());
        while let Some(id) = cur {
            let c = all.get(&id).ok_or_else(|| anyhow!("missing commit {id}"))?;
            cur = c.parents.first().cloned();
            out.push(c.clone());
        }
        out.reverse();
        Ok(out)
    }

    /// Lineage and replayed state at `tip` (HEAD when None). Empty memory gives empty state.
    pub fn state_at(&self, tip: Option<&str>) -> Result<(Vec<Commit>, State)> {
        let tip = match tip {
            Some(t) => Some(self.resolve(t)?),
            None => self.head()?,
        };
        match tip {
            None => Ok((vec![], State::default())),
            Some(t) => {
                let lineage = self.lineage(&t)?;
                let state = State::replay(&lineage);
                Ok((lineage, state))
            }
        }
    }

    /// Append a commit on the current branch.
    pub fn commit(&self, author: &str, message: &str, ops: Vec<Op>) -> Result<Commit> {
        if ops.is_empty() {
            bail!("nothing to commit: every op is already known");
        }
        let mut refs = self.refs()?;
        let parents: Vec<Id> = refs.branches.get(&refs.head).cloned().into_iter().collect();
        let commit = Commit::new(parents, author, message, now_rfc3339(), ops);
        let mut f = OpenOptions::new().append(true).create(true).open(self.log_path())?;
        writeln!(f, "{}", serde_json::to_string(&commit)?)?;
        refs.branches.insert(refs.head.clone(), commit.id.clone());
        self.save_refs(&refs)?;
        Ok(commit)
    }

    pub fn create_branch(&self, name: &str, at: Option<&str>) -> Result<()> {
        let mut refs = self.refs()?;
        if refs.branches.contains_key(name) {
            bail!("branch '{name}' already exists");
        }
        let at = match at {
            Some(spec) => self.resolve(spec)?,
            None => self.head()?.ok_or_else(|| anyhow!("no commits yet to branch from"))?,
        };
        refs.branches.insert(name.into(), at);
        self.save_refs(&refs)
    }

    pub fn switch(&self, name: &str) -> Result<()> {
        let mut refs = self.refs()?;
        if !refs.branches.contains_key(name) {
            bail!("no branch '{name}' (create it with `mneme branch {name}`)");
        }
        refs.head = name.into();
        self.save_refs(&refs)
    }

    /// Usage log: what was recalled for which question, and later whether it helped.
    /// Operational record (not memory content); the learning loop reads it.
    pub fn append_episode(&self, record: &serde_json::Value) -> Result<()> {
        let mut f = OpenOptions::new().append(true).create(true).open(self.mem().join("episodes.jsonl"))?;
        writeln!(f, "{}", serde_json::to_string(record)?)?;
        Ok(())
    }

    pub fn save_source_payload(&self, id: &str, ext: &str, content: &[u8]) -> Result<PathBuf> {
        let path = self.mem().join("sources").join(format!("{id}.{ext}"));
        if !path.exists() {
            fs::write(&path, content)?;
        }
        Ok(path)
    }
}
