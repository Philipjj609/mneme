# mneme

**A memory for you and your AI agents that knows what it knows, what it doesn't, and where each piece came from.**

Most "AI memory" is one of three things: a folder of markdown notes, a vector database, or a knowledge graph. Each is good at one thing and bad at the rest, and none of them can tell you how well something is *understood*. mneme takes a different route:

- **One append-only log of claims.** Every statement records where it came from (`observed` by a tool · `stated` by a source · `inferred` by a model, with a confidence value · `imported` · `intuited`), its stance (`is` · `should` · `could` · `canon` · `hypothetical`), its scope, when it was true, and its evidence.
- **Git-like history.** Commits are content-addressed and chained, and branches are cheap. You can `diff` two points, or rebuild the memory as it was at any commit. Nothing is overwritten: a changed value *supersedes* the old one, which stays in history.
- **Comprehension you can measure.** Each kind of thing (tool, concept, post, project…) has a *frame*: the slots that understanding it consists of (purpose, mechanism, alternatives, limits…). Filled slots are what's known. **Empty slots are named unknowns**, and they become the research agenda.
- **Every format is a view.** Graph, nested map, clusters, comprehension grid, history timeline and markdown wiki pages are all compiled from the log and rebuilt whenever you like.
- **Cheap for AI.** Agents ask through `recall`: a budgeted briefing with *why* each item is included and what is *not* known, or a one-line-per-item `--brief` view at ~25 tokens per item. Every recall is logged and can be rated, which is the signal for recall that learns.

> Status: early but working (Nov 2026). The author runs it on a personal memory of 840+ things and 2,600+ claims (GitHub stars, web tools, X posts and articles), kept in a separate private repo: this repo is the engine only. Design: [docs/DESIGN.md](docs/DESIGN.md) · Plan: [docs/PLAN.md](docs/PLAN.md).

---

## Quick start

Requires [Rust](https://rustup.rs) (stable). Works on Windows, macOS and Linux.

```bash
git clone https://github.com/Philipjj609/mneme
cd mneme && cargo build --release            # binary: target/release/mneme (mneme.exe on Windows)
```

### Start your own memory

A memory is just a folder (keep it in its own git repo, private if it's personal):

```bash
mkdir my-memory && cd my-memory
mneme init                                   # memory/ + default comprehension frames/
mneme capture github-stars --user <you>      # your starred repos, as observed facts
mneme capture links saved.txt                # any links: web pages, X posts, GitHub, Hugging Face
mneme recall "what do I have for agent memory?" --brief
mneme build                                  # projections/view.html
python -m http.server 4173 --directory projections   # open http://localhost:4173/view.html
```

mneme finds the memory via `--root <dir>`, else `$MNEME_ROOT`, else by searching upward from the current folder. So you can run it from anywhere with `export MNEME_ROOT=~/my-memory`.

`saved.txt` takes one URL per line, with an optional note after it. `# from <url>` heads a group of links that came from that post:

```
https://github.com/morluto/rea
https://x.com/someone/status/123 orchestrate sub agents
# from https://x.com/someone/status/456
https://ui.shadcn.com
```

---

## How it works

```
capture  ──►  facts (observed / stated), each with its source kept in memory/sources/
digest   ──►  interpretation from a model or a person, validated and evidence-linked
recall   ──►  a briefing for a question: recognize → follow links → package (+ unknowns)
feedback ──►  did it help? (episodes.jsonl, the learning signal)
build    ──►  graph · map · clusters · comprehension · history · wiki   (all regenerable)
```

**Capture** records only what changed. Asking GitHub again only commits new star counts, which supersede the old ones. It supports:
- GitHub stars, repos and READMEs
- web pages (title, description, readable text)
- X posts, including the full text of X articles, via the public fxtwitter API, with links inside a post followed one level
- X profiles
- Hugging Face datasets

**Digest** is how interpretation gets in. A model (or you) writes a proposal file. mneme rejects unknown subjects and id-like typos, links known ids, attaches evidence automatically, and archives the proposal as a source. The protocol for doing this well is [docs/DIGEST.md](docs/DIGEST.md). It places every item on each axis: what it is, what it provides, **at which stage of a workflow** you'd use it, access/price, peers, fit for your projects, and the lesson a post teaches.

```json
{ "author": "claude-opus-5-5", "message": "…", "basis": "inferred", "confidence": 0.7, "evidence": "auto",
  "anchors": [ { "id": "concept:agent-memory", "name": "Agent memory", "summary": "…" } ],
  "claims": [ ["repo:vectorize-io/hindsight", "provides", "concept:agent-memory", 0.95],
              { "about": "site:designmd.ai", "relation": "used_at_stage", "value": "stage:design-system" },
              { "about": "…", "relation": "mechanism", "value": "…", "basis": "stated", "evidence": ["s_…"] } ] }
```

**Recall** needs no model. It scores things by keyword relevance (BM25), then spreads along meaningful links (provides, used at stage, alternative to, needs…) weighted by confidence. It returns ranked items with the path that brought each one in, what's known slot by slot, and what's unknown. A model reads that and answers.

---

## Use it from an AI agent (Claude Code, Codex, Cursor…)

Install the skill so your agent asks the memory before searching, and writes back what it learns:

```bash
# Claude Code (per user); other agents: copy skills/mneme into their skills folder
cp -r skills/mneme ~/.claude/skills/mneme
```

The skill ([skills/mneme/SKILL.md](skills/mneme/SKILL.md)) teaches the cheap-first loop: `recall --brief`, then `page <id>` only where needed, then a full `recall` only if necessary, then rate it with `feedback`. Agents working *on* this repo follow [AGENTS.md](AGENTS.md).

---

## Commands

| Command | What it does |
|---|---|
| `init` | create `memory/` here |
| `capture github-stars --user U` | starred repos as observed facts |
| `capture links <file> [--no-follow]` | web pages, X posts/articles/profiles, GitHub repos (+README), HF datasets, with your notes |
| `capture readme owner/name …` | READMEs of repos already in memory |
| `digest <proposal.json>` | commit an interpretation (validated, evidence-linked) |
| `recall "<question>" [--brief] [--budget N]` | briefing for a question; logged as an episode |
| `feedback <episode> good\|partial\|bad [--used ids] [--missing "…"]` | rate a recall |
| `page <id>` | everything known about one thing, slot by slot |
| `frontier [--kind K]` | most-connected, least-understood things: what to research next |
| `retract <claim>… --reason "…"` | withdraw wrong claims (kept in history) |
| `status` · `log` · `show <commit>` · `diff <a> <b>` | history |
| `branch [name] [--at x]` · `switch <name>` | alternative lines of memory (what-ifs) |
| `build [--at <commit>]` | regenerate views, optionally as the memory was then |

## The explorer

`projections/view.html` gives four views of the same memory, with a time slider over commits:

- **Graph**, in three modes:
  - **Map:** nested boxes (category → concept → tools/sites/posts); edges appear only for what you select.
  - **Network:** a force layout of all links.
  - **Focus:** the selection and its neighbours, in rings.
- **Clusters:** capabilities with what provides them, and what could serve each project.
- **Comprehension:** a grid of filled vs empty slots, coloured by the strongest basis.
- **History:** every commit and what it changed.

Click anything to see what's known about it, the evidence, what's unknown, and its past values.

## Layout

A memory folder:

```
memory/log.jsonl      the memory — one commit per line (append-only)
memory/refs.json      branches + HEAD
memory/sources/       captured evidence (API payloads, pages as text, READMEs, proposals) —
                      local only, not in git: it holds copies of third-party content. Every
                      source keeps its URL and content hash in the log; re-capture to rebuild it.
memory/episodes.jsonl recall + feedback log
frames/               comprehension frames per kind of thing
proposals/            digest proposals (interpretations)
inbox/                link lists as they were pasted
```

This repo (the engine):

```
crates/mneme-core     model, log, replay, frames, digest, recall, projections, viewer
crates/mneme-cli      the `mneme` command and capture adapters
frames/               default frames (written by `mneme init`)
skills/mneme/         agent skill
docs/                 design, plan, digest protocol, tracking
```

## Honest limits

- Recall matches keywords before following links, so paraphrases ("make it feel premium") need embeddings (planned).
- Ratings are recorded, but recall doesn't learn from them yet (planned).
- Pages rendered by JavaScript capture little text. Some sites block automated fetches, and those are marked "fetch failed" rather than guessed.
- Video content in posts isn't transcribed. Posts are marked when their substance is in a video.

## License

MIT OR Apache-2.0, at your option.
