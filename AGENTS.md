# Working in the mneme repo (for AI agents)

**How we write code:** [organon/docs/ENGINEERING.md](https://github.com/Philipjj609/organon/blob/main/docs/ENGINEERING.md). Read it before changing anything.

**Using the memory** (recall, capture, digest, feedback): follow [skills/mneme/SKILL.md](skills/mneme/SKILL.md). Digest rules are in [docs/DIGEST.md](docs/DIGEST.md); where things are tracked is in [docs/TRACKING.md](docs/TRACKING.md).

**Layout:**
- `crates/mneme-core`: the library. No terminal output, no network.
  - `store`: log, refs, branches.
  - `state`: replay with indexes, plus `Batch`, which stages changes without copying.
  - `digest`: validates proposals.
  - `recall`: recognize → activate → package.
  - `frames`, `project` (views; `viewer.html` is embedded).
  - `error`: typed errors.
  - Knowledge types come from [organon-protocol](https://github.com/Philipjj609/organon-protocol).
- `crates/mneme-cli`: the `mneme` binary.
  - `commands`: renders output.
  - `links`, `github`: capture.
  - `net`: the `Fetch` trait, plus a fake for tests.

**Quality gate** (CI runs it on Linux, macOS and Windows): `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`. Every bug fix gets a regression test (see `crates/mneme-core/tests/memory.rs`).

**Test against a real memory** with `--root` or `$MNEME_ROOT` (Philip's: `../mneme-memory`). Never create or commit a memory inside this repo. Before merging a refactor, compare old and new binaries on the real memory: `status`, `recall --brief`, `build` (graph.json must be identical).

**Invariants. Do not break these:**
1. `memory/log.jsonl` is append-only. Fix mistakes with `retract` or a superseding claim.
2. Projections are generated. Never hand-edit them.
3. Interpretation enters only through `digest`. Capture records only what changed.
4. Unknown stays unknown: no invented facts, and no confidence on observed or stated claims.
5. Content ids are contracts (organon-protocol golden tests).

**End of a work session:** write a session-digest proposal (decisions, status, open questions as claims on `project:*`), digest it into the memory, and commit both repos.
