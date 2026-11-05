# How everything is tracked

Paths below (`memory/`, `proposals/`, `inbox/`) are inside the **memory folder**, which is its own repo (Philip's is the private `mneme-memory`), not inside this engine repo.

One home for each kind of thing. If you can't tell where something goes, it probably belongs in the memory as a claim.

| What | Where | How it gets there |
|---|---|---|
| **Code** (mneme itself) | git: a branch per change, merged to `main` | normal commits |
| **Knowledge** (repos, links, posts, concepts, how things work) | `memory/log.jsonl`, also in git | `mneme capture …` (facts) → `mneme digest proposals/….json` (interpretation) |
| **Decisions, status, open questions** for *any* project | claims on `project:*` in the memory (`decision`, `status`, `question`) | a **session digest**: at the end of each work session, one proposal `proposals/<date>-NN-session-*.json` |
| **Usage**: what was asked, what came back, whether it helped | `memory/episodes.jsonl` | `mneme recall` logs automatically; `mneme feedback <episode> good\|partial\|bad --used … --missing …` |
| **Why the system is shaped this way** | `docs/DESIGN.md` | rarely edited |
| **What's next** | `docs/PLAN.md` | ticked as milestones land |

## Daily loop

```bash
mneme capture …                    # bring something in (stars, READMEs; links and posts in M1)
mneme digest proposals/<file>.json # a model's interpretation, checked + evidence-linked
mneme recall "your question"       # ask; the package says what's known AND what isn't
mneme feedback <episode> good      # did it help? (trains recall later)
mneme frontier                     # what to research next
mneme build                        # refresh the views
git add -A && git commit           # memory + code, versioned together
```

## Asking about a project's state

`mneme page project:koda` shows the project's status, decisions, open questions and candidate tools, with history. Nothing lives in a loose notes file that drifts out of date.

## Rules that keep it honest

- Facts from a source are `observed`/`stated` and cite it. Synthesis is `inferred` with a confidence value.
- Nothing is overwritten. Changed values supersede old ones, which stay in history (`mneme diff`, the time slider).
- "NOT KNOWN" in a recall means an empty slot. Answer "unknown"; don't guess.
- No new doc longer than DESIGN.md unless real data has flowed through the thing it describes.
