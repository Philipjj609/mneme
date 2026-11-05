//! Network access behind a trait, so capture logic can be tested with fixtures.

use std::io::Read as _;
use std::time::Duration;

use anyhow::Context as _;

const USER_AGENT: &str =
    "Mozilla/5.0 (compatible; mneme/0.2; +https://github.com/Philipjj609/mneme)";
const MAX_BYTES: u64 = 4_000_000;

/// Fetch a URL as text.
pub trait Fetch {
    /// # Errors
    /// Network failure or a non-success status.
    fn text(&self, url: &str, accept: &str) -> anyhow::Result<String>;
}

/// Real HTTP: 25 s timeout, 4 MB cap, `GITHUB_TOKEN` sent to api.github.com only.
pub struct Http {
    agent: ureq::Agent,
    github_token: Option<String>,
}

impl Http {
    #[must_use]
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(25))
                .user_agent(USER_AGENT)
                .build(),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
        }
    }
}

impl Fetch for Http {
    fn text(&self, url: &str, accept: &str) -> anyhow::Result<String> {
        let mut req = self.agent.get(url).set("Accept", accept);
        if let (Some(token), true) = (
            &self.github_token,
            url.starts_with("https://api.github.com/"),
        ) {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        let resp = req.call().with_context(|| format!("GET {url}"))?;
        let mut buf = Vec::new();
        resp.into_reader()
            .take(MAX_BYTES)
            .read_to_end(&mut buf)
            .with_context(|| format!("reading {url}"))?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// Like [`Fetch::text`], plus the `Link` header (GitHub pagination).
pub fn github_page(http: &Http, url: &str) -> anyhow::Result<(serde_json::Value, bool)> {
    let mut req = http
        .agent
        .get(url)
        .set("Accept", "application/vnd.github.star+json");
    if let Some(token) = &http.github_token {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let resp = req.call().with_context(|| format!("GET {url}"))?;
    let has_next = resp
        .header("link")
        .is_some_and(|l| l.contains("rel=\"next\""));
    let body: serde_json::Value = serde_json::from_reader(resp.into_reader().take(MAX_BYTES))?;
    Ok((body, has_next))
}

#[cfg(test)]
pub mod fake {
    use std::collections::HashMap;

    /// Serves fixed bodies by URL; unknown URLs fail like a 404.
    #[derive(Default)]
    pub struct Fake(pub HashMap<String, String>);

    impl super::Fetch for Fake {
        fn text(&self, url: &str, _accept: &str) -> anyhow::Result<String> {
            self.0
                .get(url)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("GET {url}: status code 404"))
        }
    }
}
