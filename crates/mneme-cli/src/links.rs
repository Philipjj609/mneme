//! `mneme capture links <file>`: anything saved, by URL.
//!
//! Dispatch by URL: `x.com/<u>/status/<id>` → post (text and X article via the public fxtwitter
//! API), `x.com/<u>` → account, `github.com/<o>/<r>` → repo + README, `huggingface.co/datasets/…`
//! → dataset + card, anything else → site (title, description, readable text). Plus the saver's
//! note and group. Links inside posts are followed one level. Failures are recorded, not guessed.
//!
//! This is sense-level work and moves to organon-text later.

use std::collections::BTreeSet;

use mneme_core::model::{
    Anchor, AnchorId, Basis, Claim, ClaimBody, Op, Scope, Source, SourceId, Value,
};
use mneme_core::util::now_rfc3339;
use mneme_core::{Batch, Repo, State};
use organon_protocol::sha256_hex;
use serde_json::Value as J;

use crate::net::Fetch;

const AUTHOR: &str = "mneme/capture-links@0.2";
const PAGE_TEXT_LIMIT: usize = 6000;
const ARTICLE_TEXT_LIMIT: usize = 16000;

/// One saved link with its note and group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub url: String,
    pub note: Option<String>,
    pub group: Option<String>,
}

/// One URL per line, optional note after it; `# heading` names a group for the following
/// lines until a bare `#`.
#[must_use]
pub fn parse_file(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut group: Option<String> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(g) = line.strip_prefix('#') {
            let g = g.trim();
            group = (!g.is_empty()).then(|| g.to_owned());
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(url) = words.next().filter(|w| w.starts_with("http")) else {
            continue;
        };
        let note = words.collect::<Vec<_>>().join(" ");
        out.push(Entry {
            url: url.to_owned(),
            note: (!note.is_empty()).then_some(note),
            group: group.clone(),
        });
    }
    out
}

/// What kind of thing a URL names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Post,
    Account,
    Repo,
    Dataset,
    Site,
}

/// Canonical anchor id and kind for a URL.
#[must_use]
pub fn anchor_for(url: &str) -> (String, LinkKind) {
    let u = url.trim().trim_end_matches(['/', '.', ',', ')', ']']);
    let rest = u.split_once("://").map_or(u, |(_, r)| r);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match (host, segs.as_slice()) {
        ("x.com" | "twitter.com", [_, "status", id, ..]) => {
            (format!("post:x/{id}"), LinkKind::Post)
        }
        ("x.com" | "twitter.com", [user]) => (
            format!("account:x/{}", user.to_lowercase()),
            LinkKind::Account,
        ),
        ("github.com", [owner, repo, ..]) => (format!("repo:{owner}/{repo}"), LinkKind::Repo),
        ("huggingface.co", ["datasets", owner, name, ..]) => {
            (format!("dataset:hf/{owner}/{name}"), LinkKind::Dataset)
        }
        _ if segs.is_empty() => (format!("site:{host}"), LinkKind::Site),
        _ => (format!("site:{host}/{}", segs.join("/")), LinkKind::Site),
    }
}

/// What a small HTML reader can get from a page.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub title: Option<String>,
    pub description: Option<String>,
    pub headings: Vec<String>,
    pub text: String,
}

fn decode_entities(s: &str) -> String {
    const ENTITIES: &[(&str, &str)] = &[
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&#039;", "'"),
        ("&#x27;", "'"),
        ("&nbsp;", " "),
        ("&#8217;", "’"),
        ("&#8211;", "–"),
        ("&#8212;", "—"),
    ];
    ENTITIES
        .iter()
        .fold(s.to_owned(), |acc, (from, to)| acc.replace(from, to))
}

fn remove_blocks(html: &str, tag: &str) -> String {
    let lower = html.to_lowercase();
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while let Some(start) = lower[i..].find(&open).map(|x| x + i) {
        out.push_str(&html[i..start]);
        match lower[start..].find(&close) {
            Some(end) => i = start + end + close.len(),
            None => return out,
        }
    }
    out.push_str(&html[i..]);
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    ['"', '\''].iter().find_map(|q| {
        let key = format!("{name}={q}");
        let start = lower.find(&key)? + key.len();
        let end = tag[start..].find(*q)?;
        Some(decode_entities(tag[start..start + end].trim()))
    })
}

fn visible_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_chars(mut s: String, limit: usize) -> String {
    if s.len() > limit {
        let cut = s
            .char_indices()
            .take_while(|(i, _)| *i < limit)
            .last()
            .map_or(0, |(i, _)| i);
        s.truncate(cut);
        s.push_str(" …");
    }
    s
}

/// Title, description (meta or og/twitter), h1/h2 headings and readable text of a page.
#[must_use]
pub fn read_page(html: &str) -> Page {
    let html = ["script", "style", "noscript", "svg", "template"]
        .iter()
        .fold(html.to_owned(), |h, t| remove_blocks(&h, t));
    let lower = html.to_lowercase();
    let title = lower.find("<title").and_then(|s| {
        let start = html[s..].find('>')? + s + 1;
        let end = lower[start..].find("</title>")? + start;
        Some(decode_entities(html[start..end].trim())).filter(|t| !t.is_empty())
    });
    let (mut description, mut social) = (None, None);
    let mut i = 0;
    while let Some(p) = lower[i..].find("<meta").map(|x| x + i) {
        let end = lower[p..].find('>').map_or(lower.len(), |x| x + p);
        let tag = &html[p..end];
        let key = attribute(tag, "name")
            .or_else(|| attribute(tag, "property"))
            .unwrap_or_default()
            .to_lowercase();
        if let Some(content) = attribute(tag, "content") {
            match key.as_str() {
                "description" if description.is_none() => description = Some(content),
                "og:description" | "twitter:description" if social.is_none() => {
                    social = Some(content)
                }
                _ => {}
            }
        }
        i = end;
    }
    let mut headings = Vec::new();
    for tag in ["h1", "h2"] {
        let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
        let mut j = 0;
        while let Some(s) = lower[j..].find(&open).map(|x| x + j) {
            let Some(e) = lower[s..].find(&close).map(|x| x + s) else {
                break;
            };
            // Tags are removed entirely by `visible_text`. (v0 dropped the first word here,
            // wrongly assuming the tag name survived: every captured heading lost a word.)
            let text = visible_text(&html[s..e]);
            if !text.is_empty() && text.len() < 160 && headings.len() < 12 {
                headings.push(text);
            }
            j = e;
        }
    }
    let body = lower.find("<body").map_or(html.as_str(), |b| &html[b..]);
    Page {
        title,
        description: description.or(social),
        headings,
        text: truncate_chars(visible_text(body), PAGE_TEXT_LIMIT),
    }
}

/// URLs in free text, trailing punctuation removed, in order, without duplicates.
#[must_use]
pub fn urls_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let Some(p) = word.find("http") else { continue };
        let url = word[p..].trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '/'));
        if (url.starts_with("http://") || url.starts_with("https://"))
            && !out.iter().any(|u| u == url)
        {
            out.push(url.to_owned());
        }
    }
    out
}

fn one_line(s: &str, max_chars: usize) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max_chars {
        t
    } else {
        format!("{}…", t.chars().take(max_chars).collect::<String>())
    }
}

fn today() -> String {
    now_rfc3339()[..10].to_owned()
}

/// Capture context: staged changes plus where payloads go.
struct Capture<'a> {
    repo: &'a Repo,
    fetch: &'a dyn Fetch,
    batch: Batch<'a>,
    now: String,
}

impl Capture<'_> {
    fn source(
        &mut self,
        kind: &str,
        uri: &str,
        title: &str,
        ext: &str,
        content: &str,
    ) -> anyhow::Result<SourceId> {
        let id = SourceId::of(content.as_bytes());
        if !self.batch.has_source(&id) {
            self.repo
                .save_source_payload(id.as_str(), ext, content.as_bytes())?;
            self.batch.add_source(Source {
                id: id.clone(),
                kind: kind.to_owned(),
                uri: uri.to_owned(),
                title: title.to_owned(),
                captured_at: self.now.clone(),
                content_hash: sha256_hex(content.as_bytes()),
                files: Default::default(),
                meta: Default::default(),
                produced_by: None,
            });
        }
        Ok(id)
    }

    fn anchor(&mut self, id: &str, name: &str, summary: Option<String>) -> anyhow::Result<()> {
        let id: AnchorId = id.parse()?;
        self.batch.upsert_anchor(Anchor {
            id,
            name: name.to_owned(),
            aliases: Vec::new(),
            scope: Scope::world(),
            summary,
        });
        Ok(())
    }

    /// Placeholder anchor for something known only by URL (never renames an existing one).
    fn stub(&mut self, id: &str) -> anyhow::Result<()> {
        if self.batch.anchor(id).is_none() {
            self.anchor(id, id.split_once(':').map_or(id, |(_, n)| n), None)?;
        }
        Ok(())
    }

    /// Record a fact unless it is already known: re-observing is not news.
    fn fact(
        &mut self,
        about: &str,
        relation: &str,
        value: Value,
        basis: Basis,
        source: &SourceId,
        edit: impl FnOnce(&mut ClaimBody),
    ) -> anyhow::Result<()> {
        let about: AnchorId = about.parse()?;
        let mut body =
            Claim::draft(about, relation, value, basis, AUTHOR).with_evidence(source.clone());
        edit(&mut body);
        self.batch.assert_if_new(body.finalize());
        Ok(())
    }

    fn get(&self, url: &str, accept: &str) -> anyhow::Result<String> {
        self.fetch.text(url, accept)
    }
}

/// Fetch one URL into the batch. Returns the anchor id and the links found inside (posts).
fn fetch_one(cx: &mut Capture, url: &str) -> anyhow::Result<(String, Vec<String>)> {
    let (id, kind) = anchor_for(url);
    let links = match kind {
        LinkKind::Post => fetch_post(cx, url, &id)?,
        LinkKind::Account => fetch_account(cx, url, &id).map(|()| Vec::new())?,
        LinkKind::Repo => fetch_repo(cx, url, &id).map(|()| Vec::new())?,
        LinkKind::Dataset => fetch_dataset(cx, url, &id).map(|()| Vec::new())?,
        LinkKind::Site => fetch_site(cx, url, &id).map(|()| Vec::new())?,
    };
    Ok((id, links))
}

fn article_of(t: &J) -> Option<(String, String)> {
    let a = t["article"].as_object()?;
    let blocks: Vec<&str> = a
        .get("content")
        .and_then(|c| c["blocks"].as_array())
        .map(|bs| {
            bs.iter()
                .filter_map(|b| b["text"].as_str())
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .unwrap_or_default();
    let title = a
        .get("title")
        .and_then(J::as_str)
        .unwrap_or_default()
        .to_owned();
    Some((title, truncate_chars(blocks.join("\n"), ARTICLE_TEXT_LIMIT)))
}

fn fetch_post(cx: &mut Capture, url: &str, id: &str) -> anyhow::Result<Vec<String>> {
    let post_id = id.trim_start_matches("post:x/");
    let doc: J = serde_json::from_str(&cx.get(
        &format!("https://api.fxtwitter.com/status/{post_id}"),
        "application/json",
    )?)?;
    let t = &doc["tweet"];
    let text = t["text"].as_str().unwrap_or_default().to_owned();
    let user = t["author"]["screen_name"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    let article = article_of(t);
    let kept = serde_json::json!({
        "url": t["url"], "text": text,
        "author": { "screen_name": user, "name": t["author"]["name"], "description": t["author"]["description"], "followers": t["author"]["followers"] },
        "created_at": t["created_at"], "likes": t["likes"], "bookmarks": t["bookmarks"], "views": t["views"],
        "media": t["media"]["all"].as_array().map(|m| m.iter().map(|x| serde_json::json!({"type": x["type"], "url": x["url"]})).collect::<Vec<_>>()),
        "quote": t["quote"].as_object().map(|q| serde_json::json!({"url": q.get("url"), "text": q.get("text"), "author": q.get("author").and_then(|a| a.get("screen_name"))})),
        "card": t["twitter_card"],
        "article": article.as_ref().map(|(title, body)| serde_json::json!({ "title": title, "text": body })),
    });
    let src = cx.source(
        "x-post",
        url,
        &format!("@{user} on X"),
        "json",
        &serde_json::to_string_pretty(&kept)?,
    )?;
    let account = format!("account:x/{}", user.to_lowercase());
    cx.anchor(
        &account,
        &format!("@{user}"),
        t["author"]["description"].as_str().map(str::to_owned),
    )?;
    let headline = article
        .as_ref()
        .map(|(title, _)| title.as_str())
        .filter(|t| !t.is_empty())
        .unwrap_or(&text)
        .to_owned();
    cx.anchor(
        id,
        &format!("@{user}: {}", one_line(&headline, 60)),
        Some(one_line(&headline, 280)),
    )?;
    let created = t["created_at"].as_str().map(str::to_owned);
    cx.fact(
        id,
        "posted_by",
        Value::Anchor(account.parse()?),
        Basis::Observed,
        &src,
        |c| c.valid_from = created,
    )?;
    cx.fact(
        id,
        "says",
        Value::Text(text.clone()),
        Basis::Stated,
        &src,
        |_| {},
    )?;
    if let Some(likes) = t["likes"].as_f64() {
        cx.fact(
            id,
            "likes",
            Value::Number(likes),
            Basis::Observed,
            &src,
            |c| c.valid_from = Some(today()),
        )?;
    }
    if let Some((title, body)) = &article {
        cx.fact(
            id,
            "has_article",
            Value::Text(format!("{title} ({} chars)", body.len())),
            Basis::Observed,
            &src,
            |_| {},
        )?;
    }
    let videos = t["media"]["videos"].as_array().map_or(0, Vec::len);
    if videos > 0 {
        cx.fact(
            id,
            "has_media",
            Value::Text(format!("{videos} video(s) — content not transcribed")),
            Basis::Observed,
            &src,
            |_| {},
        )?;
    }
    let mut links = urls_in(&text);
    if let Some(q) = t["quote"]["url"].as_str() {
        links.push(q.to_owned());
    }
    Ok(links)
}

fn fetch_account(cx: &mut Capture, url: &str, id: &str) -> anyhow::Result<()> {
    let user = id.trim_start_matches("account:x/");
    let doc: J = serde_json::from_str(&cx.get(
        &format!("https://api.fxtwitter.com/{user}"),
        "application/json",
    )?)?;
    let u = &doc["user"];
    let kept = serde_json::json!({ "screen_name": u["screen_name"], "name": u["name"], "description": u["description"], "followers": u["followers"], "joined": u["joined"], "url": u["url"] });
    let src = cx.source(
        "x-account",
        url,
        &format!("@{user} profile"),
        "json",
        &serde_json::to_string_pretty(&kept)?,
    )?;
    let description = u["description"].as_str().map(str::to_owned);
    cx.anchor(
        id,
        &format!("@{}", u["screen_name"].as_str().unwrap_or(user)),
        description.clone(),
    )?;
    if let Some(d) = description {
        cx.fact(
            id,
            "described_as",
            Value::Text(d),
            Basis::Stated,
            &src,
            |_| {},
        )?;
    }
    if let Some(n) = u["followers"].as_f64() {
        cx.fact(
            id,
            "followers",
            Value::Number(n),
            Basis::Observed,
            &src,
            |c| c.valid_from = Some(today()),
        )?;
    }
    Ok(())
}

fn fetch_repo(cx: &mut Capture, url: &str, id: &str) -> anyhow::Result<()> {
    let full = id.trim_start_matches("repo:").to_owned();
    if cx.batch.anchor(id).is_none() {
        let r: J = serde_json::from_str(&cx.get(
            &format!("https://api.github.com/repos/{full}"),
            "application/vnd.github+json",
        )?)?;
        let name = r["full_name"].as_str().unwrap_or(&full).to_owned();
        let kept = serde_json::json!({ "full_name": name, "description": r["description"], "language": r["language"], "stargazers_count": r["stargazers_count"], "license": r["license"]["spdx_id"], "homepage": r["homepage"], "topics": r["topics"], "html_url": r["html_url"], "pushed_at": r["pushed_at"] });
        let src = cx.source(
            "github-repo",
            url,
            &name,
            "json",
            &serde_json::to_string_pretty(&kept)?,
        )?;
        let description = r["description"]
            .as_str()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        cx.anchor(id, &name, description.clone())?;
        if let Some(d) = description {
            cx.fact(
                id,
                "described_as",
                Value::Text(d),
                Basis::Stated,
                &src,
                |_| {},
            )?;
        }
        if let Some(l) = r["language"].as_str() {
            cx.fact(
                id,
                "language",
                Value::Text(l.to_owned()),
                Basis::Observed,
                &src,
                |_| {},
            )?;
        }
        if let Some(n) = r["stargazers_count"].as_f64() {
            cx.fact(id, "stars", Value::Number(n), Basis::Observed, &src, |c| {
                c.valid_from = Some(today())
            })?;
        }
        if let Some(l) = r["license"]["spdx_id"]
            .as_str()
            .filter(|l| *l != "NOASSERTION")
        {
            cx.fact(
                id,
                "license",
                Value::Text(l.to_owned()),
                Basis::Observed,
                &src,
                |_| {},
            )?;
        }
    }
    let documented = cx
        .batch
        .active_about(id)
        .iter()
        .any(|c| c.relation.as_str() == "documented_in");
    if !documented {
        if let Ok(md) = cx.get(
            &format!("https://api.github.com/repos/{full}/readme"),
            "application/vnd.github.raw",
        ) {
            let src = cx.source(
                "github-readme",
                &format!("https://github.com/{full}#readme"),
                &format!("{full} README"),
                "md",
                &md,
            )?;
            cx.fact(
                id,
                "documented_in",
                Value::Text(format!("README ({} chars)", md.len())),
                Basis::Observed,
                &src,
                |_| {},
            )?;
        }
    }
    Ok(())
}

fn fetch_dataset(cx: &mut Capture, url: &str, id: &str) -> anyhow::Result<()> {
    let path = id.trim_start_matches("dataset:hf/").to_owned();
    let d: J = serde_json::from_str(&cx.get(
        &format!("https://huggingface.co/api/datasets/{path}"),
        "application/json",
    )?)?;
    let card = cx
        .get(
            &format!("https://huggingface.co/datasets/{path}/raw/main/README.md"),
            "text/plain",
        )
        .unwrap_or_default();
    let card: String = card.chars().take(PAGE_TEXT_LIMIT).collect();
    let kept = serde_json::json!({ "id": d["id"], "tags": d["tags"], "downloads": d["downloads"], "likes": d["likes"], "lastModified": d["lastModified"], "gated": d["gated"], "card": card });
    let src = cx.source(
        "hf-dataset",
        url,
        &path,
        "json",
        &serde_json::to_string_pretty(&kept)?,
    )?;
    let summary = d["cardData"]["pretty_name"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| d["description"].as_str().map(|s| one_line(s, 240)));
    cx.anchor(id, &path, summary)?;
    if let Some(n) = d["downloads"].as_f64() {
        cx.fact(
            id,
            "downloads",
            Value::Number(n),
            Basis::Observed,
            &src,
            |c| c.valid_from = Some(today()),
        )?;
    }
    for license in d["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(J::as_str)
        .filter_map(|t| t.strip_prefix("license:"))
    {
        cx.fact(
            id,
            "license",
            Value::Text(license.to_owned()),
            Basis::Observed,
            &src,
            |_| {},
        )?;
    }
    Ok(())
}

fn fetch_site(cx: &mut Capture, url: &str, id: &str) -> anyhow::Result<()> {
    let page = read_page(&cx.get(url, "text/html,application/xhtml+xml")?);
    let mut md = format!(
        "# {}\n\nurl: {url}\n",
        page.title.clone().unwrap_or_default()
    );
    if let Some(d) = &page.description {
        md.push_str(&format!("description: {d}\n"));
    }
    if !page.headings.is_empty() {
        md.push_str(&format!("headings: {}\n", page.headings.join(" | ")));
    }
    md.push_str(&format!("\n{}\n", page.text));
    let host = id.trim_start_matches("site:").to_owned();
    let src = cx.source(
        "web-page",
        url,
        page.title.as_deref().unwrap_or(&host),
        "md",
        &md,
    )?;
    cx.anchor(
        id,
        &host,
        page.description.clone().or_else(|| page.title.clone()),
    )?;
    if let Some(t) = page.title {
        cx.fact(id, "title", Value::Text(t), Basis::Observed, &src, |_| {})?;
    }
    if let Some(d) = page.description {
        cx.fact(
            id,
            "described_as",
            Value::Text(d),
            Basis::Stated,
            &src,
            |_| {},
        )?;
    }
    if page.text.len() < 200 {
        cx.fact(
            id,
            "capture_status",
            Value::Text("little readable text (likely rendered by JavaScript)".into()),
            Basis::Observed,
            &src,
            |_| {},
        )?;
    }
    Ok(())
}

/// Result of a capture run.
pub struct Outcome {
    pub ops: Vec<Op>,
    pub failures: Vec<String>,
}

/// Options for [`capture`].
pub struct Options<'a> {
    /// Anchor of who saved the links, or `None` when research found them.
    pub person: Option<&'a str>,
    /// Fetch links found inside posts.
    pub follow: bool,
}

/// Capture `entries` into a batch over `state`.
///
/// # Errors
/// Storing payloads failed (fetch failures are recorded in the outcome instead).
pub fn capture(
    repo: &Repo,
    state: &State,
    fetch: &dyn Fetch,
    entries: &[Entry],
    opts: &Options,
    log: &mut dyn FnMut(&str),
) -> anyhow::Result<Outcome> {
    let mut cx = Capture {
        repo,
        fetch,
        batch: Batch::new(state),
        now: now_rfc3339(),
    };
    let listing: String = entries
        .iter()
        .map(|e| {
            format!(
                "{} {}{}\n",
                e.url,
                e.note.clone().unwrap_or_default(),
                e.group
                    .as_ref()
                    .map(|g| format!("  [{g}]"))
                    .unwrap_or_default()
            )
        })
        .collect();
    let saved = cx.source(
        "saved-links",
        "philip:pasted-links",
        &format!("{} links", entries.len()),
        "txt",
        &listing,
    )?;
    let personal = Scope::new("personal:philip");
    let mut failures = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut found: Vec<(String, String)> = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        let (id, _) = anchor_for(&entry.url);
        match fetch_one(&mut cx, &entry.url) {
            Ok((aid, links)) => {
                log(&format!("[{}/{}] {id} ok", i + 1, entries.len()));
                found.extend(links.into_iter().map(|l| (aid.clone(), l)));
                seen.insert(aid);
            }
            Err(err) => {
                log(&format!(
                    "[{}/{}] {id} FAILED: {err:#}",
                    i + 1,
                    entries.len()
                ));
                failures.push(format!("{}: {err:#}", entry.url));
                cx.stub(&id)?;
                let status = format!("fetch failed: {}", one_line(&format!("{err:#}"), 120));
                cx.fact(
                    &id,
                    "capture_status",
                    Value::Text(status),
                    Basis::Observed,
                    &saved,
                    |_| {},
                )?;
            }
        }
        if let Some(person) = opts.person {
            let scope = personal.clone();
            cx.fact(
                &id,
                "saved_by",
                Value::Anchor(person.parse()?),
                Basis::Observed,
                &saved,
                |c| {
                    c.valid_from = Some(today());
                    c.scope = scope;
                },
            )?;
        }
        if let Some(note) = &entry.note {
            let scope = personal.clone();
            cx.fact(
                &id,
                "note",
                Value::Text(note.clone()),
                Basis::Stated,
                &saved,
                |c| c.scope = scope,
            )?;
        }
        if let Some(group) = &entry.group {
            match group.strip_prefix("from ").filter(|g| g.contains("http")) {
                Some(origin) => {
                    let (origin_id, _) = anchor_for(origin.trim());
                    cx.stub(&origin_id)?;
                    cx.fact(
                        &id,
                        "listed_in",
                        Value::Anchor(origin_id.parse()?),
                        Basis::Stated,
                        &saved,
                        |_| {},
                    )?;
                }
                None => {
                    let scope = personal.clone();
                    cx.fact(
                        &id,
                        "saved_group",
                        Value::Text(group.clone()),
                        Basis::Stated,
                        &saved,
                        |c| c.scope = scope,
                    )?;
                }
            }
        }
    }

    // Links mentioned inside posts: fetch them once (if new), then link them.
    for (from, url) in found {
        let (target, _) = anchor_for(&url);
        // X article links are carried inside the post itself; t.co are redirect shells.
        if target == from || url.contains("t.co/") || url.contains("x.com/i/article") {
            continue;
        }
        if opts.follow && cx.batch.anchor(&target).is_none() && !seen.contains(&target) {
            match fetch_one(&mut cx, &url) {
                Ok(_) => log(&format!("  follow {target} ok")),
                Err(err) => log(&format!("  follow {target} FAILED: {err:#}")),
            }
            seen.insert(target.clone());
        }
        cx.stub(&target)?;
        let evidence = cx
            .batch
            .sources_supporting(&from)
            .into_iter()
            .next()
            .unwrap_or_else(|| saved.clone());
        cx.fact(
            &from,
            "links_to",
            Value::Anchor(target.parse()?),
            Basis::Observed,
            &evidence,
            |_| {},
        )?;
    }
    Ok(Outcome {
        ops: cx.batch.into_ops(),
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::Fake;

    fn relations(ops: &[Op]) -> Vec<String> {
        ops.iter()
            .filter_map(|op| match op {
                Op::AddClaim { claim } => Some(format!("{} {}", claim.about, claim.relation)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn link_file_keeps_notes_and_groups() {
        let entries = parse_file("https://a.dev/ note here\n# from https://x.com/u/status/1\nhttps://b.dev\n#\nhttps://c.dev\nnot a link\n");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].note.as_deref(), Some("note here"));
        assert_eq!(
            entries[1].group.as_deref(),
            Some("from https://x.com/u/status/1")
        );
        assert_eq!(entries[2].group, None);
    }

    #[test]
    fn urls_map_to_anchor_ids() {
        assert_eq!(
            anchor_for("https://x.com/User/status/123?s=20").0,
            "post:x/123"
        );
        assert_eq!(
            anchor_for("https://x.com/NeroPursue").0,
            "account:x/neropursue"
        );
        assert_eq!(anchor_for("https://github.com/o/r/tree/main").0, "repo:o/r");
        assert_eq!(
            anchor_for("https://huggingface.co/datasets/a/b").0,
            "dataset:hf/a/b"
        );
        assert_eq!(
            anchor_for("https://www.Scrolltide.co/").0,
            "site:scrolltide.co"
        );
        assert_eq!(
            anchor_for("https://claude.ai/code").0,
            "site:claude.ai/code"
        );
    }

    #[test]
    fn page_reader_finds_title_description_and_text() {
        let p = read_page(
            r#"<html><head><title>Land &amp; Book</title><meta name="description" content="Hand-picked &#039;sites&#039;"><script>var x="<h1>no</h1>";</script></head><body><h1 class="t">Website inspiration</h1><p>Updated daily.</p></body></html>"#,
        );
        assert_eq!(p.title.as_deref(), Some("Land & Book"));
        assert_eq!(p.description.as_deref(), Some("Hand-picked 'sites'"));
        assert_eq!(p.headings, ["Website inspiration"]);
        assert!(p.text.contains("Updated daily.") && !p.text.contains("var x"));
    }

    #[test]
    fn urls_in_text_drop_trailing_punctuation() {
        assert_eq!(
            urls_in("see http://a.dev, and (https://b.dev/x). again http://a.dev"),
            ["http://a.dev", "https://b.dev/x"]
        );
    }

    #[test]
    fn post_capture_reads_article_and_follows_links() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo::init(dir.path()).unwrap();
        let post = serde_json::json!({"tweet": {"url": "https://x.com/u/status/9", "text": "Great list: http://tool.dev", "author": {"screen_name": "U", "description": "builder"},
            "likes": 10, "article": {"title": "The guide", "content": {"blocks": [{"text": "Step one."}, {"text": "Step two."}]}}}});
        let fake = Fake(
            [
                (
                    "https://api.fxtwitter.com/status/9".to_owned(),
                    post.to_string(),
                ),
                (
                    "http://tool.dev".to_owned(),
                    "<title>Tool</title><meta name=description content='x'><body>A tool.</body>"
                        .to_owned(),
                ),
            ]
            .into(),
        );
        let mut lines = Vec::new();
        let opts = Options {
            person: Some("person:philip"),
            follow: true,
        };
        let out = capture(
            &repo,
            &State::default(),
            &fake,
            &parse_file("https://x.com/u/status/9 why I saved it"),
            &opts,
            &mut |l| lines.push(l.to_owned()),
        )
        .unwrap();
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        let rels = relations(&out.ops);
        for want in [
            "post:x/9 says",
            "post:x/9 has_article",
            "post:x/9 saved_by",
            "post:x/9 note",
            "post:x/9 links_to",
            "site:tool.dev title",
        ] {
            assert!(rels.iter().any(|r| r == want), "missing {want} in {rels:?}");
        }
        assert!(lines.iter().any(|l| l.contains("follow site:tool.dev ok")));
    }

    #[test]
    fn failures_are_recorded_not_guessed_and_research_saves_nothing_personal() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo::init(dir.path()).unwrap();
        let opts = Options {
            person: None,
            follow: false,
        };
        let out = capture(
            &repo,
            &State::default(),
            &Fake::default(),
            &parse_file("https://blocked.dev"),
            &opts,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(out.failures.len(), 1);
        assert_eq!(
            relations(&out.ops),
            ["site:blocked.dev capture_status"],
            "no saved_by when person is None"
        );
    }
}
