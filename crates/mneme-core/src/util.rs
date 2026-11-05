use sha2::{Digest, Sha256};

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// `prefix` + first 16 hex chars of the SHA-256 of `data`.
pub fn short_hash(prefix: &str, data: &[u8]) -> String {
    format!("{prefix}{}", &sha256_hex(data)[..16])
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Split an id like `repo:owner/name` into (`repo`, `owner__name`) for file paths.
pub fn path_parts(id: &str) -> (String, String) {
    let (kind, rest) = id.split_once(':').unwrap_or(("misc", id));
    let safe: String = rest
        .chars()
        .map(|c| match c {
            '/' | '\\' => '_',
            c if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' => c,
            _ => '-',
        })
        .collect();
    (kind.to_string(), safe)
}

/// Kind prefix of an id (`repo:x` -> `repo`).
pub fn kind_of(id: &str) -> &str {
    id.split_once(':').map(|(k, _)| k).unwrap_or("misc")
}
