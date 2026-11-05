# Digest protocol: turning a captured thing into understanding

`capture` stores *what a source says*. `digest` records *what it means*. Any model can follow this protocol. Output is a proposal file (see README) that mneme validates.

## Per item, in this order

1. **Identify.** What kind of thing is it (tool, gallery, library, course, post, dataset, person)? Use `instance_of` → a `concept:type-*` anchor.
2. **Read the source, not the title.** For a post, read its full text (`says`). A post that is a list or thread *is* the explanation of the links in it: harvest the per-item descriptions it gives. If the content is a video, say so (`has_media`) and don't invent its contents.
3. **State before inferring.** Things the source says go in as `basis: stated`, citing it (`evidence: [source id]`). Your synthesis goes in as `inferred`, with a confidence and a one-line rationale.
4. **Place it on every axis.** This is what makes the knowledge multi-dimensional:
   - **What**: `provides` → capability concept (reuse existing concepts; create one only if none fits).
   - **When**: `used_at_stage` → a `stage:*` of the making process (inspiration → design system → components → motion → 3D → build → verify → ship → grow).
   - **Access**: `pricing` (free / freemium / paid / open source).
   - **Peers**: `alternative_to`, `complements`, `builds_on`, but only when you can say why.
   - **Fit**: `relevant_to` → `project:*` (stance `could`), with the reason.
   - **Source**: `links_to` / `listed_in` (from capture) and the author's reliability over time.
5. **Extract the lesson** (posts): `teaches` = the transferable idea, in one sentence. `claims` = what it asserts works, which is separate from whether it is true.
6. **Mark the edges of knowledge.** Unknown slots stay empty. Hype gets a `limitation` ("claims unverified", "launch post by the author").
7. **Promote patterns to concepts.** When several items share a mechanism, write it once on the concept (`mechanism`, `failure_mode`, `constraint`) with evidence from all of them. This is where understanding compounds.

## Cost discipline

- Batch: one proposal per coherent group (e.g. "a design-resources thread"), not one per link.
- Read compact source views (title, description, headings, first ~1,500 characters), not raw pages.
- Deep-read (full README or page) only when an item is central or the frontier asks for it.
