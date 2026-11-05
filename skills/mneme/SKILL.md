---
name: mneme
description: Use the mneme memory before researching or deciding anything about tools, design, AI agents, or Philip's projects, and record what you learn back into it. Triggers - "what do we know about", "which tool for", "how do I…" questions in areas the memory covers, choosing resources for a project, and saving links, posts or findings.
---

# mneme: shared memory for agents

mneme is a CLI over a versioned claim log. Every fact carries a basis (observed / stated / inferred), evidence, and time. Ask it before you search the web, and write back what you learn.

Find the binary: `mneme` on PATH, or `target/release/mneme` in the mneme repo. Run commands from inside the mneme repo, or pass `--root <path-to-mneme>`.

## Read (cheapest first)

1. `mneme recall "<question>" --brief` gives one line per relevant thing (~25 tokens each): id, kind, % understood, summary, unknown slots.
2. `mneme page <id>` shows everything known about one thing, slot by slot. Use this only for the items you need.
3. `mneme recall "<question>" --budget 3000` is a full package with reasons, evidence bases and what is NOT known.

Rules:
- "unknown" / "NOT KNOWN" means the memory doesn't know. Say so; don't fill it in from guesswork.
- `inferred` claims carry a confidence. `stated` means a source said it, which is different from it being true.
- After using a recall, rate it: `mneme feedback <episode> good|partial|bad --used id,id --missing "…"`. This trains recall.

## Write

- Links → `mneme capture links <file>`: one URL per line plus an optional note. It handles web pages, X posts (including X articles), GitHub repos (+README) and Hugging Face datasets.
- Interpretation → write a proposal JSON and run `mneme digest <file>`. Follow `docs/DIGEST.md`: state before inferring, place each item on every axis (what / stage / access / peers / fit), extract a post's lesson, leave unknowns empty.
- Mistakes → `mneme retract <claim-id> --reason "…"`. History is kept.
- After writing: `mneme build` refreshes the views, then commit `memory/` and `proposals/` with git.

## Cost discipline

Use brief recall before full recall, and full recall before reading sources. Batch digests per group of items. Deep-read a source only when it is central or the frontier asks for it (`mneme frontier`).
