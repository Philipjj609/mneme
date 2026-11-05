# mneme — build plan

Design: [DESIGN.md](DESIGN.md). This file is the order of work. Each milestone ends with something running on real data.

## M0 — Living core (this session)
- [x] Project folder, design, plan
Status 2026-11-05: M0 done. 77 stars captured, 2 digests, 651 anchors, 1,431 claims, explorer working.

- [x] **Claim log with git-like history**: append-only `memory/log.jsonl` of content-addressed commits (parent chain), branches in `memory/refs.json`. `log`, `show`, `diff`, `branch`, `switch`, and build at any past commit.
- [x] **Data model**: Source, Anchor, Claim (basis, stance, scope, valid time, evidence, confidence), status changes (supersede, contradict…). Re-asserting a claim reinforces it (proof count). Single-valued relations supersede the old value instead of overwriting it.
- [x] **Frames**: comprehension templates per kind. Filled slots = what we understand. Empty slots = frontier.
- [x] **capture github-stars**: Philip's stars become sources plus observed claims (language, stars-at-time, topics, license, when starred).
- [x] **digest**: a model (Claude for now) writes a *proposal* file. mneme validates it, attaches evidence, and commits it. The same format will be produced automatically by a provider later.
- [x] **Projections** (regenerable, never authoritative): `graph`/`view.html`, Karpathy-style wiki pages + `index.md` + `log.md`.
- [x] **Visual explorer**: four views of the same memory (Graph, Clusters, Comprehension, History), a time slider over commits, layer and kind filters, a detail panel with evidence and frontier.

## M1 — Feed it everything Philip saved
- Capture adapters: URL/web page, X post (via Agent-Reach or a paste), GitHub repo README, local file/folder, Drive doc.
- Digest proposals for each batch. Review the graph together.
- `mneme frontier`: rank empty slots by importance into a research agenda.
- Push to GitHub so the other machine can clone it.

## M2 — Recall that learns
- `mneme recall "<task>"` → compiled context package (mandatory set, ranked items with typed paths, omissions, manifest), sized to a token budget.
- MCP server (`mneme serve`) so Claude Code, Codex and Hermes share it.
- `feedback`: which items were used or helped → weights. Misses become links.
- BM25 (tantivy) + embeddings as entry-point indexes. Graph expansion by frame relevance.

## M3 — Growth
- `consolidate` (sleep): observations from episodes, abstraction proposals, standing pages.
- Automatic digest via provider (any model, BYOK). Research agents work the frontier.
- Skills as first-class objects, compiled from claims, flagged stale on change.
- Merge between branches; frames stored in the log itself.
- Benchmark on Philip's own corpus vs plain RAG and a wiki (same budget, blind grading).
