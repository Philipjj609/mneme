# A memory that comprehends — design proposal v0.1

> 2026-11-05. Brainstorm output, not canon. Working name for the system: **Mneme** (provisional, swap freely).

---

## 0. What this is built on

**Read fully or in the relevant parts:**
- `Projects/graphrag`: README, Project Bible §1–10 (data model, weight layers, persistence, temporal, semantic toggle, merging).
- `Projects/project-operating-system`: DECISIONS, CELL, MEMORY_ENGINE_V0_2, SELF_AUDIT_2026-07-03. This is the earlier organism attempt.
- `Projects/Kalyth`: README, ADR 0005 (claims/facets), 0011 (context packages), 0024 (reconciliation).
- `Projects/memory-architecture-lab`, `compendium`, `real-artificial-intelligence-lab`: briefs, plus PROJECTS_CATALOG.
- Drive **Universal Intelligence — Living Method & Architecture**: all section headings. Read in full: the learning model, transformation selector, frontier expansion, multidimensional knowledge, runtime loop, dual attention and epistemic self-location sections. Benchmarks 15–21 were skimmed.
- Drive **Althara Ecosystem**: about 1.2M characters. I read the vision, Layer 3 (universal semantic model), Layer 4 epistemic layers and Layer 5 (memory/context) in full. Layers 6–11 (agency, views, Code/Nexus/CAD/robotics apps) were only outlined. **Most of that document is not read.**
- All 77 GitHub stars by title and description. READMEs of hindsight, PageIndex, claude-mem, billion-context, xerj, ASI-Evolve and ECC (memory parts).
- The external field: Karpathy LLM Wiki, Graphiti/Zep, Mem0, Letta, A-MEM, HippoRAG, REALM (2026), and the "budgeted context restoration" critique of memory leaderboards (2026).

---

## 1. The field in one table

| System | Memory is… | Strong at | Breaks on |
|---|---|---|---|
| Markdown folders / CLAUDE.md | text files a model reads | human-readable, zero infra | no structure, doesn't scale, rots silently |
| **Karpathy LLM Wiki** | raw sources → LLM-maintained wiki pages + `index.md` + `log.md`; ops: ingest / query / lint | compounding synthesis, contradictions flagged at ingest, no vectors needed up to ~400k words | the page is the unit, so claims inside pages have no provenance, time or confidence; doesn't scale far |
| Vector RAG | chunks + embeddings | fuzzy entry points | similarity ≠ relevance; no structure, time or contradiction |
| PageIndex | tree index + LLM reasoning over it | long structured documents, traceable | per-document; not a memory that learns |
| MS GraphRAG / LightRAG | entity graph + community summaries | global "what are the themes" questions | expensive to build, static |
| HippoRAG | graph + personalized PageRank (hippocampal index) | multi-hop association | facts only, no epistemics |
| **Graphiti / Zep** | bi-temporal knowledge graph | "what was true when", no LLM at query time | triples are thin; no notion of understanding |
| Mem0 | extract → conflict-check → update; user/session/agent scopes | simple write path | shallow, fact-level |
| Letta (MemGPT) | pinned core blocks + searchable archive + sleep-time compute | self-managed context | memory ≈ text blocks |
| **Hindsight** (your star) | world facts / experiences / **observations** (consolidated beliefs with proof counts) / **mental models** (standing answers, rewritten in the background); retain / recall / reflect; 4 retrievers fused | learns beliefs over time; boot from settled pages; small decision models | no explicit understanding structure, no skills link |
| claude-mem | captured observations + progressive disclosure (index → timeline → detail) | token-cheap recall | session log, not knowledge |
| ECC instincts | confidence-scored patterns injected at session start, clustered into skills | procedural learning | not tied to the *why* behind a pattern |
| REALM (2026) | reconsolidation: memories updated by retrieval feedback | recall becomes learning | research prototype |
| ASI-Evolve | cognition store + experiment database | learning from trials | domain-specific loop |

**Field verdict:** systems are converging on a *lifecycle* (extract → consolidate → update → forget) and on *hybrid retrieval*. Two things are still mostly missing:
1. **A representation of understanding**, as opposed to facts or pages.
2. **Learning from use**: whether a recalled memory actually helped.

## 2. What your own work already discovered

Your own work already contains most of the right pieces. They just sit in four different projects:

- **GraphRAG** has the engine: an event-sourced weighted property graph, edge layers (structural / behavioral / semantic / spatial / temporal) that can be toggled, time travel, MCP, Rust. What it lacks is epistemics.
- **Kalyth** has the most correct *knowledge atom* anywhere in your work:
  - claims carry a `basis` (observed / inferred / stated / imported) and a `stance` (as-is / to-be);
  - evidence is pinned to source spans, and evidence groups track freshness;
  - staleness propagates to everything derived from a claim;
  - proposals stay separate from commits;
  - **context packages** have a mandatory set, ranked optional items, an omissions list and fetch handles.
- **Althara** has the *kernel and the scoping laws*:
  - a small set of universal primitives;
  - identity ≠ manifestation ≠ artifact;
  - relations carry their own context, time and provenance;
  - memory classes and scopes;
  - "memory persists, context is compiled";
  - semantic level-of-detail;
  - deletion as a semantic operation;
  - epistemic layers 0–7.
- **Universal Intelligence** has the *learning theory*:
  - the learning engine runs detect → preserve → interpret → select transformations → integrate → operationalize → revise;
  - the transformation portfolio is target × operation × commitment;
  - **seed-to-neighborhood frontier expansion**;
  - "commit narrow, inquire wide";
  - recognition → activation → investigation;
  - dual attention (model-guided and anomaly-guided);
  - **epistemic self-location** (comprehend / predict / explain / act / inquire as separate readinesses).

## 3. Diagnosis: why it still feels dispersive

1. **The format war is the wrong axis.** Markdown, graph and vector are *indexes and views*, not memory. Every system crowns one as the truth and bolts the others on. Your sense that it is "dispersive" comes from this.
2. **Memory stores statements, not understanding.** A triple, a chunk or a wiki page records *what was said*. Your auth example in Universal Intelligence shows that understanding is a **shape**: purpose, mechanism, constraints, threats, recovery, alternatives, applicability. Nothing stores that shape, so nothing can rebuild it in a new context.
3. **Weak write path.** Writing usually means "extract facts". The real operations (connect, distinguish, abstract, specialize, invalidate, open a question, proceduralize) are rarely first-class.
4. **Retrieval is similarity, unbudgeted, and doesn't learn.** Nobody records whether what was recalled actually helped, or what *should* have been recalled.
5. **Skills are cut off from knowledge.** A skill is crystallized understanding. When the understanding changes, the skill should go stale, and today it doesn't.
6. **Static nodes.** New learning overwrites old learning instead of adding a scoped, timed layer.
7. **Heavy governance kills use.** The previous organism (POS) had tiers, councils and promotion gates before it had data. Memory that is costly to write never fills.
8. **Spec explosion, honestly.** Althara is about 1.2M characters, POS about 640KB of markdown, and the Universal Intelligence benchmarks drifted into synthetic lineage puzzles. A memory system only gets smarter from *real data flowing through it*. The design below is shaped so it is useful on day 1 and grows by use, not by specification.

---

## 4. The proposal

### Thesis
> **One append-only log of claims. Everything else is a compiled view of it. The system measures what it understands, notices what it doesn't, and learns from every use.**

Markdown pages, the graph, vector and BM25 indexes, PageIndex-style trees, the wiki and skill files are all *projections*. They are regenerated, never authoritative. The format war disappears: you get every format, and none of them is the memory.

### 4.1 The data model (deliberately small)

```
Source     immutable evidence: file, URL, transcript, conversation, repo@commit, X post.
           Content hash + addressable spans.

Anchor     identity only: id, kind, names/aliases, home scope.
           (concept, person, project, function, character, place, repo, technique…)

Claim      THE ATOM. A qualified statement:
             about:      [anchor | claim]          ← claims about claims and relations are allowed
             relation:   typed (from a registry; domains add types)
             value:      anchor | literal | text
             scope:      world | domain:x | project:koda | personal:philip | canon:syntropy | play
             valid_time / recorded_time            ← bi-temporal (Graphiti)
             basis:      observed | stated | inferred | imported | intuited
             stance:     is | should | could | canon | hypothetical
             confidence + rationale (inferred only)
             evidence:   groups of Source spans    ← Kalyth support semantics
             derived_from: [claim]                 ← staleness propagates down this
             status:     active | superseded | contradicted | parked
             author:     human | model@version, in which episode

Frame      a comprehension template for a kind of anchor: a set of SLOTS, each one a relation family
           (purpose, mechanism, parts, constraints, failure modes, adversary, recovery,
            alternatives, applicability, lifecycle, interfaces, human experience, verification…).
           Frames are learned and versioned. Domains and projects grow their own.

Episode    one unit of work: task, the context package used, actions, outcome, what was cited.

Skill      a procedure + applicability conditions + compiled_from [claims] + outcome record.
```

Everything that happens is an **event** in the log (GraphRAG's WAL and event-log design, Kalyth's revisions).

### 4.2 Comprehension, made operational

This is the core new idea. For any anchor, **its frame's slots filled by claims form its comprehension profile**:

- **Filled slots**, and by what basis: observed > stated > inferred > intuited.
- **Demonstrated capabilities**, which are separate axes, not one score (Universal Intelligence, Benchmark 14):
  - *recognize*: can identify it in new material;
  - *explain*: purpose and mechanism slots are filled and supported;
  - *bound*: constraints, applicability and failure modes are filled;
  - *act*: a skill or realization built from it has produced a verified outcome;
  - *generate*: it has transferred successfully to a new context, or produced a new viable alternative.
- **Empty slots are the frontier.** They are not missing data; they are named unknowns. "We know passwords. We have no claims about recovery flows or threat models." That is Universal Intelligence's seed-to-neighborhood expansion, turned into a queryable structure.

This answers "what does comprehension mean for a model" in a way you can measure. The system can say *"I can explain X but I have never acted on it"*, or *"I have used Y three times, but its failure modes slot is empty."*

### 4.3 The six operations

| Op | What it does | Cost |
|---|---|---|
| **capture** | Store any Source instantly (file, link, starred repo, X post, transcript). No model needed. | ~free |
| **digest** | A model reads the source and *proposes* a transformation portfolio: new anchors and claims, frame-slot fills, connect / distinguish / abstract / specialize / invalidate / open-question. Low-risk proposals auto-commit. **Only** scope promotions and contradictions of well-supported claims queue for review. | model call per source |
| **compile** (recall) | *recognize* the task type → *activate* relevant regions → *investigate* gaps. See 4.4. | cheap, mostly no LLM |
| **feedback** (reconsolidation) | After an episode, record which items were used or cited, whether the outcome was good, and what was missing and fetched later. Adjust edge weights and relevance. A miss becomes a new link. | free (logged) |
| **consolidate** (sleep) | Cluster episodes into *observations* (Hindsight). Propose abstractions only when ≥2 independent cases support them. Refresh standing pages only where something changed. Recompute comprehension profiles. | batch, idle time |
| **lint** | Find orphans, stale derivations, contradictions, and **important anchors with empty slots** → research agenda. | batch |

**`lint` → research agenda → research agents → `capture` → `digest`** is how the system starts doing research on its own. The frontier tells it *what* to look for, ranked by how central the anchor is and how often it gets recalled.

### 4.4 Compiling context (budgeted, explained, learnable)

1. **Entry points** come from fused retrievers: BM25, vector, entity lookup, temporal, and tree navigation for long sources (PageIndex). They are combined with rank fusion.
2. **Expansion** follows the graph by *frame relevance*, not similarity. A "rename a column" task activates the *interfaces/consumers* slot (the Universal Intelligence example). Expansion uses personalized PageRank (HippoRAG), weighted by learned usefulness from feedback.
3. **The package** (Kalyth ADR 0011, generalized):
   - a **mandatory set**: active constraints, accepted decisions, your values and authority rules for this scope;
   - **ranked optional items**, each with its typed path ("why is this here");
   - an **omissions list** with fetch handles;
   - a **manifest** of what was consulted.
   It is sized to the *target model's* budget. The same memory serves Opus, a local model or Codex.
4. **Standing pages** load first and cost nothing: Hindsight's "mental models", Karpathy's wiki pages. Examples: "Who is Philip / how he works", "Koda architecture now". They are generated projections, and reading one is a file read.

### 4.5 Scopes instead of separate systems

The Compendium, project memory and personal memory are **not three systems**. They are scopes in one substrate (Althara Layer 5):
- `world` / `domain:*`: Compendium knowledge;
- `project:*`: Koda, Lucky Impact, Kalyth…;
- `canon:*`: Syntropy truths that are true *in the story*;
- `personal:philip`: preferences and beliefs, attributed to you;
- `play`: counterfactuals.

A claim crosses scopes only through an explicit bridge (reference, promotion, abstraction). The Althara epistemic layers 0–7 map to `scope × basis × stance`, so they come for free instead of needing a separate mechanism.

### 4.6 Knowledge ↔ skill continuum

A skill records `compiled_from: [claims]` and its outcome history. When a supporting claim is superseded, the skill is flagged **stale** (Kalyth's freshness propagation). External skill repos you starred (agent-skills, impeccable, hallmark, superpowers…) are *digested*: the procedure becomes a Skill, its reasoning becomes claims with `basis: imported`, and its applicability conditions get recorded. ECC-style instincts are simply skills at low commitment.

### 4.7 One substrate, many views

| View | Built from | Who uses it | Rebuilt when |
|---|---|---|---|
| Markdown wiki pages (Karpathy) | claims per anchor + frame | you, any model, Obsidian | an anchor's claims change |
| Graph (GraphRAG engine) | anchors + claims as edges, weight layers | traversal, blast radius, Althara canvas | incrementally, every event |
| BM25 + vector indexes | claim text + sources | entry points | incrementally |
| Tree index (PageIndex) | long sources | reading books/papers precisely | at ingest |
| Skill files (`SKILL.md`) | Skill objects | Claude Code / Codex / any harness | skill changes |
| Comprehension map | frames + profiles | "what do we know vs not" | after consolidate |

---

## 5. What is genuinely new and what is borrowed

**Borrowed (credit where due):** bi-temporal claims (Graphiti); basis, evidence spans and context packages (Kalyth, i.e. you); the universal kernel and scopes (Althara, i.e. you); observations and standing pages (Hindsight); the wiki plus lint (Karpathy); tree navigation (PageIndex); PPR association (HippoRAG); reconsolidation (REALM); progressive disclosure (claude-mem).

**The new combination I'm proposing:**
1. **Comprehension frames**, with the empty slot as a first-class frontier and separate demonstrated-capability axes. To my knowledge, no shipped memory system represents *how deeply* something is understood.
2. **Recall-as-write**: feedback from real task outcomes trains retrieval and expansion (the Universal Intelligence discovery-policy learner, made concrete).
3. **Every store is a projection of one claim log**, which ends the markdown/graph/vector split.
4. **Skills tied to the claims they encode**, with staleness propagation.
5. **The frontier drives autonomous research.**

I won't claim "nobody has done this". It's a hypothesis to test (see 7).

## 6. Risks and open questions

- **Frame bootstrapping.** Who writes the first frames? Proposal: a dozen hand-written universal frames (concept, mechanism, technique, person, project, codebase component, story entity). After that, models propose domain frames and they get versioned.
- **Digest quality and cost.** Proposals from cheap models can pollute memory. Mitigation: basis and confidence are recorded; low-tier inferences stay `inferred` and never count as evidence. Contradiction checks are run against observed claims.
- **Over-expansion.** Frontier and expansion can balloon. Stop criterion: does this uncertainty change the next allowed action? (Universal Intelligence's blocking vs exploratory roles.)
- **Tacit knowledge.** Design taste, for example, isn't propositional. Allow *exemplar* claims ("these 12 references = good; these = slop") as evaluator data.
- **Privacy and deletion.** Semantic deletion via `derived_from` is designed in from the start (Althara Layer 5).

## 7. How we would know it works (before building big)

Pre-registered and budgeted (from the "budgeted context restoration" critique):
- **Corpus**: your own material. GraphRAG, Kalyth, POS, the Althara and Universal Intelligence docs, the starred repos.
- **Test set**: 40 held-out questions and tasks, from "why did Kalyth choose X" to "build sign-in for Koda the right way".
- **Baselines**: (a) plain vector RAG, (b) a Karpathy-style wiki, (c) Mneme. All get the **same token budget** and the same model.
- **Metrics**: answer quality (blind-graded), citation correctness, the *calibrated "I don't know"* rate, tokens used.
- **The real test, repeated over weeks**: does performance improve with use (feedback), without anything being re-specified?

## 8. Build shape (a lean, not a decision; your constraints come first)

- **Rust core**, reusing GraphRAG: `graph-core` persistence ideas, `graph-parser` for code ingestion, `graph-mcp`, `graph-temporal`. Kalyth's claim/evidence schema gets ported in.
- **Storage**: SQLite as the event log and materialized tables (it is what both Kalyth and POS settled on). `tantivy` for BM25, `sqlite-vec` or `usearch` for vectors, `petgraph` in memory.
- **Interfaces**: MCP + CLI, so Claude Code, Codex, Hermes and local models all share the same memory. Markdown projection goes to a folder you can open in Obsidian.
- **Day-1 slice**: capture + digest + compile + markdown projection on your own corpus. Feedback, consolidation, frames and the research loop come next, in that order.
- **Rule**: no spec document longer than this one until real data has been flowing for two weeks.
