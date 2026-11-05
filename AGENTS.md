# Working in the mneme repo (for AI agents)

**Using the memory** (recall, capture, digest, feedback): follow [skills/mneme/SKILL.md](skills/mneme/SKILL.md). Digest rules: [docs/DIGEST.md](docs/DIGEST.md). Where things are tracked: [docs/TRACKING.md](docs/TRACKING.md).

**Changing the code:**
- `crates/mneme-core` covers model, log/refs, state replay, frames, digest validation, recall, projections, and the viewer (`src/viewer.html`, embedded at build time).
- `crates/mneme-cli` is the `mneme` command plus capture adapters (`capture.rs` for GitHub, `links.rs` for web/X/HF).
- Build with `cargo build --release`. Test against a memory with `--root` or `$MNEME_ROOT` (Philip's: `../mneme-memory`). Never create or commit a memory inside this repo.

**Invariants. Do not break these:**
1. `memory/log.jsonl` is append-only. Never edit or rewrite it. Fix mistakes with `retract` or a superseding claim.
2. Projections (`projections/`) are generated. Never hand-edit them.
3. Interpretation enters only through `mneme digest` proposals. Capture records only what changed.
4. Unknown stays unknown: no invented facts, and no confidence on observed/stated claims.

**End of a work session:** write `proposals/<date>-NN-session-*.json` with the decisions, status and open questions as claims on `project:*`, digest it, then commit.
