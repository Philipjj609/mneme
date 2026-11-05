# mneme

**A memory that comprehends.** One append-only log of claims, with git-like history. Everything else (graph, wiki pages, indexes, the visual explorer) is a view compiled from that log.

- **Claims, not notes.** Every statement records where it comes from (`observed · stated · inferred · imported · intuited`), its stance (`is · should · could · canon · hypothetical`), its scope, when it was true, its evidence and its confidence.
- **History like git.** Commits are content-addressed and chained, and branches are cheap. You can `diff` two points, or rebuild the memory as it was at any commit. Nothing is overwritten: a new value *supersedes* the old one, and the old one stays in history.
- **Comprehension you can measure.** Each kind of thing has a *frame*, a list of what understanding it involves (purpose, mechanism, alternatives, limits…). Filled slots are what we know. **Empty slots are the frontier**, which becomes the research agenda.
- **Many views of one memory.** Graph, clusters, comprehension map, history, and Karpathy-style markdown pages. All regenerable.

Design: [docs/DESIGN.md](docs/DESIGN.md) · Plan: [docs/PLAN.md](docs/PLAN.md)

## Use

```bash
cargo build --release            # binary: target/release/mneme(.exe)
mneme init                       # creates memory/
mneme capture github-stars --user Philipjj609
mneme digest proposals/<file>.json   # a model's interpretation, validated + evidence-linked
mneme build                      # projections/view.html, projections/wiki/, graph.json
```

To browse, open `projections/view.html` (it needs internet once, for the graph library), or serve it:
`python -m http.server 4173 --directory projections` → http://localhost:4173/view.html

| Command | What it does |
|---|---|
| `status` | branch, head, counts |
| `log`, `show <commit>` | history and what a commit did |
| `diff <a> <b>` | what changed between commits or branches |
| `branch [name] [--at x]`, `switch <name>` | alternative lines of memory (what-ifs, experiments) |
| `build --at <commit>` | views of the memory as it was then |
| `page <anchor>` | what is known about one thing, slot by slot |
| `frontier [--kind repo]` | most connected, least understood things: the research agenda |

## Layout

```
memory/log.jsonl      the memory — one commit per line (commit this to git)
memory/refs.json      branches + HEAD
memory/sources/       raw captured evidence (API payloads, proposals)
frames/               comprehension frames per kind
proposals/            digest proposals (model-written interpretations)
projections/          generated views (gitignored; rebuild any time)
crates/mneme-core     model, log, state replay, frames, digest, projections, viewer
crates/mneme-cli      the `mneme` command, capture adapters
```

## Digest proposals

Interpretation is written as a proposal, not straight into memory:

```json
{ "author": "claude-opus-5-5", "message": "…", "basis": "inferred", "confidence": 0.7, "evidence": "auto",
  "anchors": [ { "id": "concept:agent-memory", "name": "Agent memory", "summary": "…" } ],
  "claims": [ ["repo:vectorize-io/hindsight", "provides", "concept:agent-memory", 0.95],
              { "about": "…", "relation": "verdict", "value": "…", "basis": "stated", "stance": "should" } ] }
```

mneme rejects unknown subjects. A string that names a known anchor becomes a link. `"evidence": "auto"` cites the observed sources behind both ends of a claim. The proposal itself is archived as a source. Later, any model will produce this same format automatically.
