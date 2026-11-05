//! `mneme capture links <file>`: anything Philip saved, by URL.
//!
//! Each link becomes an anchor with its own evidence, dispatched by kind:
//!   x.com/<user>/status/<id> → post (full text via the public fxtwitter API), author, links inside it
//!   x.com/<user>             → account
//!   github.com/<owner>/<repo>→ repo (+ README)
//!   huggingface.co/datasets/…→ dataset (+ card)
//!   anything else            → site (title, description, readable text)
//! Plus Philip's own note and when he saved it. Links found inside posts are followed one level.
//!
//! File format: one link per line, optional note after it; `# heading` lines name a group
//! (e.g. `# from x.com/…/status/…`), applied to the links below it until a blank `#` line.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use mneme_core::util::{now_rfc3339, sha256_hex, short_hash};
use mneme_core::*;
use serde_json::Value as J;

const AUTHOR: &str = "mneme/capture-links@0.1";
const UA: &str = "Mozilla/5.0 (compatible; mneme/0.1; +https://github.com/Philipjj609/mneme)";
const PAGE_TEXT_LIMIT: usize = 6000;

pub struct Entry {
    pub url: String,
    pub note: Option<String>,
    pub group: Option<String>,
}

pub fn parse_file(text: &str) -> Vec<Entry> {
    let mut out = vec![];
    let mut group: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(g) = l.strip_prefix('#') {
            let g = g.trim();
            group = if g.is_empty() { None } else { Some(g.to_string()) };
            continue;
        }
        let mut words = l.split_whitespace();
        let Some(first) = words.next() else { continue };
        if !first.starts_with("http") {
            continue;
        }
        let note: String = words.collect::<Vec<_>>().join(" ");
        out.push(Entry { url: first.to_string(), note: (!note.is_empty()).then_some(note), group: group.clone() });
    }
    out
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(25)).user_agent(UA).build()
}

fn get_text(url: &str, accept: &str) -> Result<String> {
    let mut req = agent().get(url).set("Accept", accept);
    if url.contains("api.github.com") {
        if let Ok(t) = std::env::var("GITHUB_TOKEN") {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
    }
    let resp = req.call().with_context(|| format!("GET {url}"))?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut resp.into_reader().take(4_000_000), &mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
use std::io::Read as _;

/// Canonical anchor id for a URL.
pub fn anchor_for(url: &str) -> (String, String) {
    let u = url.trim().trim_end_matches(['/', '.', ',', ')', ']']);
    let rest = u.split_once("://").map(|x| x.1).unwrap_or(u);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match host.as_str() {
        "x.com" | "twitter.com" if segs.len() >= 3 && segs[1] == "status" => {
            (format!("post:x/{}", segs[2]), "post".into())
        }
        "x.com" | "twitter.com" if segs.len() == 1 => (format!("account:x/{}", segs[0].to_lowercase()), "account".into()),
        "github.com" if segs.len() >= 2 => (format!("repo:{}/{}", segs[0], segs[1]), "repo".into()),
        "huggingface.co" if segs.len() >= 3 && segs[0] == "datasets" => {
            (format!("dataset:hf/{}/{}", segs[1], segs[2]), "dataset".into())
        }
        _ => {
            let p = if segs.is_empty() { String::new() } else { format!("/{}", segs.join("/")) };
            (format!("site:{host}{p}"), "site".into())
        }
    }
}

/// Very small HTML reader: title, meta description / og:*, headings, readable text.
pub struct Page {
    pub title: Option<String>,
    pub description: Option<String>,
    pub headings: Vec<String>,
    pub text: String,
}

fn decode(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
        .replace("&#39;", "'").replace("&#039;", "'").replace("&#x27;", "'").replace("&nbsp;", " ").replace("&#8217;", "’")
        .replace("&#8211;", "–").replace("&#8212;", "—")
}

fn strip_blocks(html: &str, tag: &str) -> String {
    let lower = html.to_lowercase();
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while let Some(s) = lower[i..].find(&open).map(|x| x + i) {
        out.push_str(&html[i..s]);
        match lower[s..].find(&close) {
            Some(e) => i = s + e + close.len(),
            None => {
                i = html.len();
                break;
            }
        }
    }
    if i < html.len() {
        out.push_str(&html[i..]);
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    for q in ['"', '\''] {
        let key = format!("{name}={q}");
        if let Some(p) = lower.find(&key) {
            let start = p + key.len();
            if let Some(end) = tag[start..].find(q) {
                return Some(decode(tag[start..start + end].trim()));
            }
        }
    }
    None
}

fn tags_text(html: &str) -> String {
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
    decode(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn read_page(html: &str) -> Page {
    let mut h = html.to_string();
    for t in ["script", "style", "noscript", "svg", "template"] {
        h = strip_blocks(&h, t);
    }
    let lower = h.to_lowercase();
    let title = lower.find("<title").and_then(|s| {
        let s2 = h[s..].find('>')? + s + 1;
        let e = lower[s2..].find("</title>")? + s2;
        Some(decode(h[s2..e].trim())).filter(|t| !t.is_empty())
    });
    let mut description = None;
    let mut og = None;
    let mut i = 0;
    while let Some(p) = lower[i..].find("<meta").map(|x| x + i) {
        let end = lower[p..].find('>').map(|x| x + p).unwrap_or(lower.len());
        let tag = &h[p..end];
        let key = attr(tag, "name").or_else(|| attr(tag, "property")).unwrap_or_default().to_lowercase();
        if let Some(c) = attr(tag, "content") {
            if key == "description" && description.is_none() {
                description = Some(c);
            } else if (key == "og:description" || key == "twitter:description") && og.is_none() {
                og = Some(c);
            }
        }
        i = end;
    }
    let mut headings = vec![];
    for tag in ["h1", "h2"] {
        let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
        let mut j = 0;
        while let Some(s) = lower[j..].find(&open).map(|x| x + j) {
            let Some(e) = lower[s..].find(&close).map(|x| x + s) else { break };
            let t = tags_text(&h[s..e]);
            let t = t.trim_start_matches(|c: char| c != ' ').trim().to_string();
            if !t.is_empty() && t.len() < 160 && headings.len() < 12 {
                headings.push(t);
            }
            j = e;
        }
    }
    let body = lower.find("<body").map(|b| &h[b..]).unwrap_or(&h);
    let mut text = tags_text(body);
    if text.len() > PAGE_TEXT_LIMIT {
        let cut = text.char_indices().take_while(|(i, _)| *i < PAGE_TEXT_LIMIT).last().map(|x| x.0).unwrap_or(0);
        text.truncate(cut);
        text.push_str(" …");
    }
    Page { title, description: description.or(og), headings, text }
}

pub fn urls_in(text: &str) -> Vec<String> {
    let mut out = vec![];
    for w in text.split(|c: char| c.is_whitespace()) {
        if let Some(p) = w.find("http") {
            let u = w[p..].trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '/'));
            if u.starts_with("http://") || u.starts_with("https://") {
                if !out.contains(&u.to_string()) {
                    out.push(u.to_string());
                }
            }
        }
    }
    out
}

struct Ctx<'a> {
    repo: &'a Repo,
    sim: State,
    ops: Vec<Op>,
    now: String,
    failures: Vec<String>,
}

impl Ctx<'_> {
    fn push(&mut self, op: Op) {
        self.sim.apply(&op, usize::MAX);
        self.ops.push(op);
    }
    fn anchor(&mut self, id: &str, kind: &str, name: &str, summary: Option<String>) {
        let changed = match self.sim.anchors.get(id) {
            None => true,
            Some(a) => summary.is_some() && a.anchor.summary != summary,
        };
        if changed {
            self.push(Op::AddAnchor { anchor: Anchor {
                id: id.into(), kind: kind.into(), name: name.into(), aliases: vec![],
                scope: "world".into(), summary } });
        }
    }
    fn source(&mut self, kind: &str, uri: &str, title: &str, ext: &str, content: &str) -> Result<String> {
        let sid = short_hash("s_", content.as_bytes());
        if !self.sim.sources.contains_key(&sid) {
            self.repo.save_source_payload(&sid, ext, content.as_bytes())?;
            self.push(Op::AddSource { source: Source {
                id: sid.clone(), kind: kind.into(), uri: uri.into(), title: title.into(),
                captured_at: self.now.clone(), content_hash: sha256_hex(content.as_bytes()), meta: Default::default() } });
        }
        Ok(sid)
    }
    fn claim(&mut self, about: &str, rel: &str, v: Value, basis: Basis, src: &str, f: impl FnOnce(&mut Claim)) {
        let mut c = Claim::new(about, rel, v, basis, AUTHOR).with_evidence(src);
        f(&mut c);
        let c = c.finalize();
        if self.sim.claims.get(&c.id).is_some_and(|k| k.status == Status::Active) {
            return;
        }
        for op in self.sim.assert_ops(c) {
            self.push(op);
        }
    }
}

fn shorten(s: &str, n: usize) -> String {
    let t: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n { t } else { format!("{}…", t.chars().take(n).collect::<String>()) }
}

/// Fetch one URL into the memory. Returns the anchor id and links found inside it (for posts).
fn fetch(cx: &mut Ctx, url: &str) -> Result<(String, Vec<String>)> {
    let (id, kind) = anchor_for(url);
    match kind.as_str() {
        "post" => {
            let pid = id.trim_start_matches("post:x/");
            let raw = get_text(&format!("https://api.fxtwitter.com/status/{pid}"), "application/json")?;
            let j: J = serde_json::from_str(&raw)?;
            let t = &j["tweet"];
            let text = t["text"].as_str().unwrap_or_default().to_string();
            let user = t["author"]["screen_name"].as_str().unwrap_or("unknown").to_string();
            // X "articles" carry the real content of many posts: keep title + text blocks.
            let article = t["article"].as_object().map(|a| {
                let body: Vec<&str> = a.get("content").and_then(|c| c["blocks"].as_array())
                    .map(|bs| bs.iter().filter_map(|b| b["text"].as_str()).filter(|s| !s.trim().is_empty()).collect())
                    .unwrap_or_default();
                let mut body = body.join("\n");
                if body.len() > 16000 {
                    let cut = body.char_indices().take_while(|(i, _)| *i < 16000).last().map(|x| x.0).unwrap_or(0);
                    body.truncate(cut);
                    body.push_str(" …");
                }
                (a.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string(), body)
            });
            // keep only what matters: the words, who, when, reach, media kinds, quoted post
            let keep = serde_json::json!({
                "url": t["url"], "text": text, "author": { "screen_name": user, "name": t["author"]["name"],
                "description": t["author"]["description"], "followers": t["author"]["followers"] },
                "created_at": t["created_at"], "likes": t["likes"], "bookmarks": t["bookmarks"], "views": t["views"],
                "media": t["media"]["all"].as_array().map(|m| m.iter().map(|x| serde_json::json!({"type": x["type"], "url": x["url"]})).collect::<Vec<_>>()),
                "quote": t["quote"].as_object().map(|q| serde_json::json!({"url": q.get("url"), "text": q.get("text"), "author": q.get("author").and_then(|a| a.get("screen_name"))})),
                "card": t["twitter_card"],
                "article": article.as_ref().map(|(title, body)| serde_json::json!({ "title": title, "text": body })),
            });
            let src = cx.source("x-post", url, &format!("@{user} on X"), "json", &serde_json::to_string_pretty(&keep)?)?;
            let aid = format!("account:x/{}", user.to_lowercase());
            cx.anchor(&aid, "account", &format!("@{user}"), t["author"]["description"].as_str().map(String::from));
            let headline = match &article {
                Some((title, _)) if !title.is_empty() => title.clone(),
                _ => text.clone(),
            };
            cx.anchor(&id, "post", &format!("@{user}: {}", shorten(&headline, 60)), Some(shorten(&headline, 280)));
            if let Some((title, body)) = &article {
                cx.claim(&id, "has_article", Value::Text(format!("{title} ({} chars)", body.len())), Basis::Observed, &src, |_| {});
            }
            cx.claim(&id, "posted_by", Value::Anchor(aid), Basis::Observed, &src, |c| {
                c.valid_from = t["created_at"].as_str().map(String::from);
            });
            cx.claim(&id, "says", Value::Text(text.clone()), Basis::Stated, &src, |_| {});
            if let Some(n) = t["likes"].as_f64() {
                cx.claim(&id, "likes", Value::Number(n), Basis::Observed, &src, |c| c.valid_from = Some(cx_today()));
            }
            let videos = t["media"]["videos"].as_array().map(|v| v.len()).unwrap_or(0);
            if videos > 0 {
                cx.claim(&id, "has_media", Value::Text(format!("{videos} video(s) — content not transcribed")), Basis::Observed, &src, |_| {});
            }
            let mut links = urls_in(&text);
            if let Some(q) = t["quote"]["url"].as_str() {
                links.push(q.to_string());
            }
            Ok((id, links))
        }
        "account" => {
            let user = id.trim_start_matches("account:x/");
            let raw = get_text(&format!("https://api.fxtwitter.com/{user}"), "application/json")?;
            let j: J = serde_json::from_str(&raw)?;
            let u = &j["user"];
            let keep = serde_json::json!({ "screen_name": u["screen_name"], "name": u["name"], "description": u["description"],
                "followers": u["followers"], "joined": u["joined"], "url": u["url"] });
            let src = cx.source("x-account", url, &format!("@{user} profile"), "json", &serde_json::to_string_pretty(&keep)?)?;
            let desc = u["description"].as_str().map(String::from);
            cx.anchor(&id, "account", &format!("@{}", u["screen_name"].as_str().unwrap_or(user)), desc.clone());
            if let Some(d) = desc {
                cx.claim(&id, "described_as", Value::Text(d), Basis::Stated, &src, |_| {});
            }
            if let Some(n) = u["followers"].as_f64() {
                cx.claim(&id, "followers", Value::Number(n), Basis::Observed, &src, |c| c.valid_from = Some(cx_today()));
            }
            Ok((id, vec![]))
        }
        "repo" => {
            let full = id.trim_start_matches("repo:").to_string();
            if !cx.sim.anchors.contains_key(&id) {
                let raw = get_text(&format!("https://api.github.com/repos/{full}"), "application/vnd.github+json")?;
                let r: J = serde_json::from_str(&raw)?;
                let name = r["full_name"].as_str().unwrap_or(&full).to_string();
                let keep = serde_json::json!({ "full_name": name, "description": r["description"], "language": r["language"],
                    "stargazers_count": r["stargazers_count"], "license": r["license"]["spdx_id"], "homepage": r["homepage"],
                    "topics": r["topics"], "html_url": r["html_url"], "pushed_at": r["pushed_at"] });
                let src = cx.source("github-repo", url, &name, "json", &serde_json::to_string_pretty(&keep)?)?;
                let desc = r["description"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                cx.anchor(&id, "repo", &name, desc.clone());
                if let Some(d) = desc {
                    cx.claim(&id, "described_as", Value::Text(d), Basis::Stated, &src, |_| {});
                }
                if let Some(l) = r["language"].as_str() {
                    cx.claim(&id, "language", Value::Text(l.into()), Basis::Observed, &src, |_| {});
                }
                if let Some(n) = r["stargazers_count"].as_f64() {
                    cx.claim(&id, "stars", Value::Number(n), Basis::Observed, &src, |c| c.valid_from = Some(cx_today()));
                }
                if let Some(l) = r["license"]["spdx_id"].as_str().filter(|l| *l != "NOASSERTION") {
                    cx.claim(&id, "license", Value::Text(l.into()), Basis::Observed, &src, |_| {});
                }
            }
            if !cx.sim.about(&id).any(|c| c.claim.relation == "documented_in") {
                if let Ok(md) = get_text(&format!("https://api.github.com/repos/{full}/readme"), "application/vnd.github.raw") {
                    let src = cx.source("github-readme", &format!("https://github.com/{full}#readme"), &format!("{full} README"), "md", &md)?;
                    cx.claim(&id, "documented_in", Value::Text(format!("README ({} chars)", md.len())), Basis::Observed, &src, |_| {});
                }
            }
            Ok((id, vec![]))
        }
        "dataset" => {
            let path = id.trim_start_matches("dataset:hf/").to_string();
            let raw = get_text(&format!("https://huggingface.co/api/datasets/{path}"), "application/json")?;
            let d: J = serde_json::from_str(&raw)?;
            let card = get_text(&format!("https://huggingface.co/datasets/{path}/raw/main/README.md"), "text/plain").unwrap_or_default();
            let card: String = card.chars().take(PAGE_TEXT_LIMIT).collect();
            let keep = serde_json::json!({ "id": d["id"], "tags": d["tags"], "downloads": d["downloads"], "likes": d["likes"],
                "lastModified": d["lastModified"], "gated": d["gated"], "card": card });
            let src = cx.source("hf-dataset", url, &path, "json", &serde_json::to_string_pretty(&keep)?)?;
            let desc = d["cardData"]["pretty_name"].as_str().map(String::from)
                .or_else(|| d["description"].as_str().map(|s| shorten(s, 240)));
            cx.anchor(&id, "dataset", &path, desc);
            if let Some(n) = d["downloads"].as_f64() {
                cx.claim(&id, "downloads", Value::Number(n), Basis::Observed, &src, |c| c.valid_from = Some(cx_today()));
            }
            for tag in d["tags"].as_array().into_iter().flatten().filter_map(|t| t.as_str()) {
                if let Some(l) = tag.strip_prefix("license:") {
                    cx.claim(&id, "license", Value::Text(l.into()), Basis::Observed, &src, |_| {});
                }
            }
            Ok((id, vec![]))
        }
        _ => {
            let html = get_text(url, "text/html,application/xhtml+xml")?;
            let p = read_page(&html);
            let mut md = format!("# {}\n\nurl: {url}\n", p.title.clone().unwrap_or_default());
            if let Some(d) = &p.description {
                md.push_str(&format!("description: {d}\n"));
            }
            if !p.headings.is_empty() {
                md.push_str(&format!("headings: {}\n", p.headings.join(" | ")));
            }
            md.push_str(&format!("\n{}\n", p.text));
            let host = id.trim_start_matches("site:").to_string();
            let src = cx.source("web-page", url, p.title.as_deref().unwrap_or(&host), "md", &md)?;
            let name = host.clone();
            cx.anchor(&id, "site", &name, p.description.clone().or(p.title.clone()));
            if let Some(t) = &p.title {
                cx.claim(&id, "title", Value::Text(t.clone()), Basis::Observed, &src, |_| {});
            }
            if let Some(d) = p.description {
                cx.claim(&id, "described_as", Value::Text(d), Basis::Stated, &src, |_| {});
            }
            if p.text.len() < 200 {
                cx.claim(&id, "capture_status", Value::Text("little readable text (likely rendered by JavaScript)".into()), Basis::Observed, &src, |_| {});
            }
            Ok((id, vec![]))
        }
    }
}

fn cx_today() -> String {
    now_rfc3339()[..10].to_string()
}

pub fn capture(repo: &Repo, state: &State, entries: &[Entry], person: &str, follow: bool) -> Result<(Vec<Op>, Vec<String>)> {
    let mut cx = Ctx { repo, sim: state.clone(), ops: vec![], now: now_rfc3339(), failures: vec![] };
    let personal_src = {
        let listing: String = entries.iter().map(|e| {
            format!("{} {}{}\n", e.url, e.note.clone().unwrap_or_default(), e.group.as_ref().map(|g| format!("  [{g}]")).unwrap_or_default())
        }).collect();
        cx.source("saved-links", "philip:pasted-links", &format!("{} links saved by Philip", entries.len()), "txt", &listing)?
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut follow_queue: Vec<(String, String)> = vec![];
    for (i, e) in entries.iter().enumerate() {
        let (id, kind) = anchor_for(&e.url);
        eprint!("[{}/{}] {} ", i + 1, entries.len(), id);
        match fetch(&mut cx, &e.url) {
            Ok((aid, links)) => {
                eprintln!("ok");
                seen.insert(aid.clone());
                for l in links {
                    follow_queue.push((aid.clone(), l));
                }
            }
            Err(err) => {
                eprintln!("FAILED: {err:#}");
                cx.failures.push(format!("{}: {err:#}", e.url));
                cx.anchor(&id, &kind, id.split_once(':').map(|x| x.1).unwrap_or(&id), None);
                cx.claim(&id, "capture_status", Value::Text(format!("fetch failed: {}", shorten(&format!("{err:#}"), 120))), Basis::Observed, &personal_src, |_| {});
            }
        }
        // `--person none`: found by research, not saved by a person.
        if person != "none" {
            cx.claim(&id, "saved_by", Value::Anchor(person.into()), Basis::Observed, &personal_src, |c| {
                c.valid_from = Some(cx_today());
                c.scope = "personal:philip".into();
            });
        }
        if let Some(n) = &e.note {
            cx.claim(&id, "note", Value::Text(n.clone()), Basis::Stated, &personal_src, |c| c.scope = "personal:philip".into());
        }
        if let Some(g) = &e.group {
            let (gid, _) = anchor_for(g.trim_start_matches("from ").trim());
            if g.contains("http") || g.starts_with("from ") {
                cx.claim(&id, "listed_in", Value::Anchor(gid), Basis::Stated, &personal_src, |_| {});
            } else {
                cx.claim(&id, "saved_group", Value::Text(g.clone()), Basis::Stated, &personal_src, |c| c.scope = "personal:philip".into());
            }
        }
    }
    // Links mentioned inside posts: link them, and fetch them once if we don't know them yet.
    for (from, url) in follow_queue {
        let (tid, tkind) = anchor_for(&url);
        // X article links are carried inside the post itself (article text), not separate pages.
        if tid == from || url.contains("t.co/") || url.contains("x.com/i/article") {
            continue;
        }
        if follow && !cx.sim.anchors.contains_key(&tid) && !seen.contains(&tid) {
            eprint!("  follow {tid} ");
            match fetch(&mut cx, &url) {
                Ok(_) => eprintln!("ok"),
                Err(err) => {
                    eprintln!("FAILED: {err:#}");
                    cx.anchor(&tid, &tkind, tid.split_once(':').map(|x| x.1).unwrap_or(&tid), None);
                }
            }
            seen.insert(tid.clone());
        } else if !cx.sim.anchors.contains_key(&tid) {
            cx.anchor(&tid, &tkind, tid.split_once(':').map(|x| x.1).unwrap_or(&tid), None);
        }
        let src = cx.sim.sources_supporting(&from).into_iter().next().unwrap_or(personal_src.clone());
        cx.claim(&from, "links_to", Value::Anchor(tid), Basis::Observed, &src, |_| {});
    }
    Ok((cx.ops, cx.failures))
}
