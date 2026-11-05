/// Current UTC time, RFC 3339, second precision.
#[must_use]
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Split an id like `repo:owner/name` into (`repo`, `owner_name`) for file paths.
#[must_use]
pub fn path_parts(id: &str) -> (String, String) {
    let (kind, rest) = id.split_once(':').unwrap_or(("misc", id));
    let safe = rest
        .chars()
        .map(|c| match c {
            '/' | '\\' => '_',
            c if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') => c,
            _ => '-',
        })
        .collect();
    (kind.to_owned(), safe)
}

#[cfg(test)]
mod tests {
    #[test]
    fn path_parts_are_filesystem_safe() {
        assert_eq!(
            super::path_parts("repo:owner/name"),
            ("repo".into(), "owner_name".into())
        );
        assert_eq!(
            super::path_parts("site:x.com/a?b"),
            ("site".into(), "x.com_a-b".into())
        );
    }
}
