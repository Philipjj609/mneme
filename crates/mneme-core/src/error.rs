use std::path::PathBuf;

/// Everything that can go wrong in the memory engine. Callers can match on it; the CLI adds
/// context and prints it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("{path} line {line}: {source}")]
    Parse {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },

    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error(
        "no mneme memory found from {0} upward (run `mneme init`, or set --root / MNEME_ROOT)"
    )]
    NoMemory(PathBuf),

    #[error("no branch or commit matches '{0}'")]
    UnknownRevision(String),

    #[error("'{spec}' is ambiguous ({count} commits match)")]
    AmbiguousRevision { spec: String, count: usize },

    #[error("the memory has no commits yet")]
    Empty,

    #[error("nothing to commit: every op is already known")]
    NothingToCommit,

    #[error("branch '{0}' already exists")]
    BranchExists(String),

    #[error("no branch '{0}' (create it with `mneme branch {0}`)")]
    NoSuchBranch(String),

    #[error("log refers to missing commit {0}")]
    MissingCommit(String),

    #[error("proposal rejected:\n  {}", .0.join("\n  "))]
    Rejected(Vec<String>),

    #[error(transparent)]
    Id(#[from] organon_protocol::IdError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }
}
