# Codesynapse planning docs — index

> **TL;DR:** 7 planning docs (Rounds 1-5) covering 98 directions for beating codegraph. Read this index first, then dive into the relevant doc.

---

## Quick reference table

| Doc | Lines | Directions | Round | Theme | Read time |
|---|---|---|---|---|---|
| [`codesynapse-vs-codegraph-analysis.md`](codesynapse-vs-codegraph-analysis.md) | 142 | 4 | 0 (original) | First diagnosis: 2-gap root cause | 5 min |
| [`direction-a-b-implementation-plan.md`](direction-a-b-implementation-plan.md) | 666 | A+B + 4 (8) | 1.5 | **Start here for execution.** A+B is the 3-4h quick win | 20 min |
| [`winning-against-codegraph.md`](winning-against-codegraph.md) | 540 | 28 | 1 | Benchmark accuracy directions | 30 min |
| [`making-codesynapse-better.md`](making-codesynapse-better.md) | 376 | 19 | 2 | Product improvements (perf, UX, features) | 20 min |
| [`strategy-against-codegraph.md`](strategy-against-codegraph.md) | 239 | 8 | 2 | Strategic plays + positioning | 15 min |
| [`deep-technical-wins.md`](deep-technical-wins.md) | 476 | 26 (25 + Part 9 source body) | 3 | Technical deep dive + 10 surgical code fixes | 30 min |
| [`advanced-retrieval.md`](advanced-retrieval.md) | 224 | 11 | 4 | Algorithmic + architectural shifts | 15 min |
| [`round-5-meta-robustness.md`](round-5-meta-robustness.md) | 168 | 7 | 5 | Meta + robustness + new capabilities | 10 min |
| [`opencode-analysis-2026-06-06.md`](opencode-analysis-2026-06-06.md) | (new) | 1 + corrections | 5.5 | **NEW root cause:** Source body indexing. Post-verification analysis. | 10 min |
| [`opencode-pushback-2026-06-06.md`](opencode-pushback-2026-06-06.md) | 415 | 4 pushbacks + responses | 5.6 | **opencode pushback + opencode response to claude analysis.** 4 pushbacks, 4 responses, 2 NEW FINDINGS accepted (Direction 14 blocker, Direction 5 sequencing). FINAL STAND section is the comprehensive plan reference. | 10 min |
| **[`../IMPLEMENTATION_PLAN.md`](../IMPLEMENTATION_PLAN.md)** | — | — | **FINAL** | **Start here to implement.** Phase-by-phase plan with file:line targets, benchmark commands, and decision gates. | 10 min |
| **Total** | **3,375+** | **99** | | | ~2.5h |

---

## When to read which doc

| If you want to... | Read this doc first |
|---|---|
| **Execute something today** (3-4h quick win) | `direction-a-b-implementation-plan.md` (start with A+B) |
| **Improve benchmark accuracy** (the core question) | `winning-against-codegraph.md` → `deep-technical-wins.md` → `advanced-retrieval.md` |
| **Find the single highest-leverage change** | `advanced-retrieval.md` Part 4 (Top 5 to execute) |
| **Find cheap code-level fixes** | `deep-technical-wins.md` Part 4 (Issues 1-10 with file:line refs) |
| **Position codesynapse vs codegraph** | `strategy-against-codegraph.md` |
| **Improve product (not just accuracy)** | `making-codesynapse-better.md` |
| **Add new capabilities** (pattern detection, time queries) | `round-5-meta-robustness.md` (T16, T17) |
| **Understand the original diagnosis** | `codesynapse-vs-codegraph-analysis.md` |
| **Understand the codebase** | `ARCHITECTURE.md`, `MCP-TOOLS.md` |

---

## Execution priority (the recommended path)

> **opencode analysis (2026-06-06):** priority list updated after Claude's code verification + my analysis. **Source body indexing is the new #1 priority** — the actual root cause of the django-fts failure, not Direction 1 (docstrings). See `opencode-analysis-2026-06-06.md` for the full token-overlap analysis.
>
> > **opencode pushback (2026-06-06):** the priority list above may flip. After Claude's audit, the realistic gain from source body is +5-15pp (dense-side, variable by query type), not +15-25pp. **Direction 14 (codesage-large 356M model upgrade)** from `winning-against-codegraph.md` may be a higher-leverage next move. Updated priority list with Direction 14 as Phase 2a: see `opencode-pushback-2026-06-06.md` § "What this changes about the recommendation."
> >
> > > **opencode response to claude audit (2026-06-06):** Validation-gate approach adopted. Test source body first (1d, $0, no model uncertainty) → decision gate → Direction 14 only if validated. Same total cost in success case, strictly lower in failure case, strictly more information. **The "no" path saves 2-3 days.** See `opencode-pushback-2026-06-06.md` § "My FINAL recommendation" for the full plan.
> >
> > > **opencode response to claude analysis of responses (2026-06-06, FINAL):** Two NEW FINDINGS from Claude: (1) Direction 14 has architectural blocker — `StaticEmbedder` is Model2Vec-only (verified `embedding.rs:543-545`); (2) Direction 5 is wrongly sequenced as conditional fallback — it's orthogonal to source body. **Both accepted.** Updated plan: Direction 5 in Phase 2a parallel to source body (not as fallback). Direction 14 updated cost: 1d pre-distilled check OR 1-2d Path A (distill) OR 3-5d Path B (transformer inference). **Total Phase 2: 5-9d (yes path) or 3-4d (no path).** See `opencode-pushback-2026-06-06.md` § "FINAL STAND" for full plan.

### Phase 1: Today (1-4h)
1. **Direction A+B** from `direction-a-b-implementation-plan.md` — soften prompt + low-conf hint. ~3-4h. Re-run benchmark.
2. **Issues 1, 6, 7, 9** from `deep-technical-wins.md` Part 4 — BM25 weight bug, hard method cap, `top_k=3`, symbol extraction. ~40 min total. Re-run benchmark.
   - ⚠️ **Skip Issue 10** — code-verified false positive. Re-sort in `apply_score_adjustments` is correct behavior.
   - Issue 8 largely dissolves after Issue 2 (BM25 cache) is done — low independent priority.

### Phase 2a: This week, Days 1-2 (1-2d) — TWO PARALLEL TRACKS [opencode response to claude analysis 2026-06-06]
**Track A (1-2d):** **Direction 5** (`codesynapse_context` tool) from `winning-against-codegraph.md:118-127` — single-tool parity with codegraph, full version (symbol extraction + 1-hop graph expansion). 1-2d. **Proven mechanism** — codegraph won django-fts (10/10, 1 call) via this exact approach. **High confidence.**
**Track B (1d):** **Source body Step 1** — broken-fallback as benchmark experiment, gated behind `--source-body-experiment` CLI flag (off by default). 1d, $0 API, no model uncertainty. Re-benchmark django-fts.
   - **Decision gate (after Track B):** does source body improve django-fts by +5-15pp?
   - Yes path → Phase 2b (Source body Step 2) + Phase 2c (Direction 14)
   - No path → Phase 2c (Direction 14) skipped, focus on Direction 5 + other directions
   - **Both tracks run in parallel; Direction 5 is the proven mechanism, source body is the speculative validation.**

### Phase 2b: This week, Day 2-3 (1d) — IF source body Track B VALIDATED
2. **Source body Step 2** — fix language extractors (Python at minimum) to store start/end line numbers. Reuse `outline_items` path (already extracts line ranges at `mcp.rs:2817`). Remove `--source-body-experiment` flag. Production correctness for long files.

### Phase 2c: This week, Day 2-6 (1-5d) — IF source body Track B VALIDATED
3. **Direction 14 (model upgrade)** from `winning-against-codegraph.md:209-220` — has architectural blocker per Claude's NEW FINDING: `StaticEmbedder` is Model2Vec-only (verified `embedding.rs:543-545`).
   - **Step 1 (1d):** Check if pre-distilled larger Model2Vec code model exists (e.g., `minishlab/potion-code-76M` or similar). Drop-in if found.
   - **Step 2a (Path A, 1-2d):** Distill codesage-large → Model2Vec. Loses transformer semantics.
   - **Step 2b (Path B, 3-5d):** Add transformer inference (candle or ort Rust dep). Real transformer semantics.
   - **Recommendation:** Do Step 1 first. If drop-in found, no transformer work needed. If not, decide between Path A (cheap, lossy) and Path B (expensive, real work).

### Phase 3: Next week (1 week)
4. **Caching fixes** (Issues 2, 3, 4 in `deep-technical-wins.md`) — graph + BM25 + source caches. ~3h. Significant wall-time win.
5. **Tech 5** (Doc2Query) from `deep-technical-wins.md` — embed the questions the user will ask. ~2 days. Compounds with source body indexing.

### Phase 4: Following week (1-2 weeks)
6. **T1** (ColBERT) from `advanced-retrieval.md` — token-level matching. ~3-4 days.
7. **T2** (Concept hierarchy) from `advanced-retrieval.md` — intent-driven search. ~2-3 days.
8. **Tech 1** (HNSW) from `deep-technical-wins.md` — fast dense search. ~1 day.
9. **Tech 3** (Two-stage retrieval) from `deep-technical-wins.md` — BM25 on source + dense on labels. ~2 days.
10. **Tech 8** (Server-side decomposition) from `deep-technical-wins.md` — multi-hop in 1 call. ~2 days.

**Target after Phase 4:** 9-9.5/10 on FTS5-fail rows (from current 4.5/10).

**Stop point (2026-06-06):** This is the final plan. 3 rounds of back-and-forth (opencode analysis → claude analysis → opencode pushback → claude audit → opencode response → claude analysis of responses → opencode final). **No more analysis cycles. Time to execute.**

---

## Direction counts by doc

| Doc | Direction count | ID range |
|---|---|---|
| `codesynapse-vs-codegraph-analysis.md` | 4 | 1-4 |
| `direction-a-b-implementation-plan.md` | 8 (A+B + 1-4 + 1-4 added later) | A, B, 1-4 |
| `winning-against-codegraph.md` | 28 | 5-28 |
| `making-codesynapse-better.md` | 19 | 29-47 |
| `strategy-against-codegraph.md` | 8 | S1-S8 |
| `deep-technical-wins.md` | 25 | Tech 1-15 + Issues 1-10 |
| `advanced-retrieval.md` | 11 | T1-T11 |
| `round-5-meta-robustness.md` | 7 | T12-T18 |
| **Total** | **~98** | |

(Note: ID ranges are approximate. The strategic and product directions use S-prefix and a different numbering scheme.)

---

## The cross-cutting pattern (from `advanced-retrieval.md` Part 5)

1. **The query path is fine.** The model picks tools well. The problem is the *content available to retrieve*.
2. **The gap is at index time, not query time.** Doc2Query, concept extraction, commit mining, test fixtures all enrich the index.
3. **Dense search is the main lever, but only if you have the right thing to embed.** Embedding the *code* misses; embedding the *question* (Doc2Query), *test* (T3), *commit message* (T4), or *concept* (T2) lands.
4. **Multi-hop questions dominate.** Single-shot `resolve()` is a structural limitation. Tech 8 (decomposition), T2 (concept hierarchy), T7 (subgraph sketching) are the three fixes at different layers.
5. **ColBERT > single-vector for code.** Token-level matching is the next architectural shift.
6. **The most strategic value is positioning/docs/distribution, not new code.** ~10 of 98 directions need real code work. The other 88 are prompt, config, doc, distribution.
7. **Operational quality is its own dimension.** T13, T14, T15, T18 don't improve retrieval directly — they make the system more robust and learnable.
8. **New capabilities beat better existing ones (after a point).** T16, T17 don't improve existing queries — they unlock *new queries*.

---

## Reading order for a new agent

**If you have 5 minutes:**
- Read this index doc
- Skim the "Cross-cutting pattern" above

**If you have 30 minutes:**
- Read `codesynapse-vs-codegraph-analysis.md` (original 4-direction analysis)
- Read `direction-a-b-implementation-plan.md` (current A+B plan)
- Read `advanced-retrieval.md` Part 4 (Top 5 to execute)

**If you have 2 hours:**
- Read all 7 planning docs in order: 0 → 1.5 → 1 → 2 → 3 → 4 → 5
- Skip the verbose Parts 6-8 of each (open questions, diminishing returns)

**If you have 4 hours:**
- Read every planning doc end-to-end
- Read `ARCHITECTURE.md` and `MCP-TOOLS.md` for the codebase
- Start executing Phase 1 of the priority list above

---

## Related project docs (non-planning)

These describe the *code*, not the *plan*. Read for codebase understanding, not for what to build.

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — 343 lines. How the Rust crates fit together. Crate responsibilities, data flow.
- [`MCP-TOOLS.md`](MCP-TOOLS.md) — 409 lines. All MCP tools, their inputs/outputs, when to use them. The "user manual" for an agent using codesynapse.
- `../BENCHMARKS.md` — How the benchmark works. Question format, judge, scoring.
- `../BENCHMARKS-OPENCODE.md` — Benchmark run instructions.
- `../FUTURE_ENHANCEMENTS.md` — Docstring extraction work (prereq for Direction 1 in `winning-against-codegraph.md`); status: Done for Python/Java/JS/TS.
- `../README.md` — Top-level project README.
- `../bench/questions.tsv` — 16 current benchmark questions.
- `../bench/judge.py` — Judge script (target of Direction 26 in `winning-against-codegraph.md`).
- [`opencode-analysis-2026-06-06.md`](opencode-analysis-2026-06-06.md) — **NEW (opencode analysis 2026-06-06).** Post-verification analysis. Identifies source body indexing as the actual root cause of the django-fts failure. Token-overlap analysis + new priority list.
- [`opencode-pushback-2026-06-06.md`](opencode-pushback-2026-06-06.md) — **NEW (opencode pushback 2026-06-06 + opencode response to claude audit 2026-06-06 + opencode response to claude analysis of responses 2026-06-06).** 4 pushbacks, 4 responses, 2 NEW FINDINGS accepted (Direction 14 architectural blocker; Direction 5 wrongly sequenced). **FINAL STAND** section captures the final plan: Direction 5 (1-2d) + Source body Step 1 (1d) in parallel as Phase 2a, then Step 2 + Direction 14 gated on source body result. Direction 14 cost: 1d pre-distilled check OR 1-2d Path A OR 3-5d Path B. **Stop point: no more analysis cycles.**

---

## Key code references (file:line)

For an agent that needs to know where to make changes:

| Direction | File:line | Action |
|---|---|---|
| Direction A (prompt) | `codesynapse-mcp/src/mcp.rs:56-96` | Soften the system prompt |
| Direction B (low-conf) | `codesynapse-mcp/src/mcp.rs:2775-2847` | Add confidence hint to `tool_codesynapse_resolve` |
| Issue 1 (BM25 bug) | `codesynapse-serve/src/graph_query.rs:575-576` | Add 3rd signal to RRF |
| Issue 2 (Bm25 cache) | `codesynapse-serve/src/graph_query.rs:558, 1281` | Cache Bm25Index on ServeGraph |
| Issue 3 (graph cache) | `codesynapse-mcp/src/mcp.rs:2787, 1735` | Cache ServeGraph in McpServer |
| Issue 4 (source cache) | `codesynapse-mcp/src/mcp.rs:2813, 2818` | Cache source files + outlines |
| Issue 5 (brute-force) | `codesynapse-serve/src/graph_query.rs:569-572` | Replace with HNSW (Tech 1) |
| **Source body indexing (opencode analysis 2026-06-06)** | `codesynapse-serve/src/graph_query.rs:1086-1100` + `codesynapse-core/src/global_graph.rs:416-429` | **NEW #1 priority (per opencode analysis).** [opencode response to claude audit 2026-06-06: validation-gate approach — 1d experiment first, then decide]. Realistic gain: +5-15pp dense-side (variable by query type). Direct fix for django-fts. Two-step plan: Step 1 (broken-fallback, gated) → Step 2 (fix extractors). |
| Issue 6 (method cap) | `codesynapse-mcp/src/mcp.rs:2849` | Replace hard 8 with budget check |
| Issue 7 (top_k=5) | `codesynapse-mcp/src/mcp.rs:2784` | Change default to 3 |
| Issue 9 (symbol extract) | `codesynapse-serve/src/graph_query.rs:1203-1212` | Split identifiers into tokens |
| Direction 1 (docstring) | `codesynapse-core/src/types.rs` | Add `docstring: Option<String>` to Node |
| Direction 18 (ToolDef audit) | `codesynapse-mcp/src/mcp.rs:109-...` | Audit all 24 tool descriptions |
| **Direction 14 (codesage-large 356M) [opencode response: do AFTER source body experiment validates]** | `codesynapse-core/src/embedding.rs` | Upgrade embedder from `potion-code-16M` to `codesage-large` (356M params, attention-based). [opencode response to claude audit 2026-06-06: cost is 2-3 days, not 1-2; +1d for query-embedding cache due to 50-200ms/call latency]. Run AFTER source body validates, not before. ~1GB model download. |
| Direction 21 (LLM summary) | `codesynapse-core/src/ts_extract/*.rs` | LLM-call at build time per node |
| Direction 26 (multi-judge) | `bench/judge.py` | Multi-judge averaging |
| Direction 32 (doctor) | `codesynapse-cli/src/cli.rs` | New `codesynapse doctor` subcommand |
| ~~Issue 10 (re-sort)~~ | ~~`graph_query.rs:1261`~~ | ~~Remove re-sort~~ — **FALSE POSITIVE**: re-sort is correct, do not implement |

---

## Code-verification status (cross-checked against source)

**`deep-technical-wins.md`**
- Issues 1, 2, 3, 4, 6, 7, 9 — confirmed real (code read, bug present).
- Issue 10 — false positive (re-sort after score adjustments is correct behavior).
- Issue 8 — low priority (dissolves after Issue 2 cache fix).

**`direction-a-b-implementation-plan.md`**
- Direction A — plausible (`mcp.rs:56-96` uses hard "NEVER"/"HARD LIMITS" language).
- Direction B — confirmed gap (no confidence signal in resolve output).

**`winning-against-codegraph.md`**
- Root-cause claim "embedding uses just label + source_file" — **WRONG**. Both BM25 (`graph_query.rs:1080-1091`) and dense (`global_graph.rs:421-428`) already embed `label + docstring + source_file + child_method_names`. The actual missing signal is **source code body text**.
- Direction 1 ("add docstrings to embedding") — **already done** for BM25 + dense + Python/Java/JS/TS extraction. Remaining work: Rust/Go docstrings + indexing source body.
- Direction 5 (`codesynapse_context` tool) — confirmed not implemented. Highest-priority new feature.

**`advanced-retrieval.md`**
- T18 silent dense fallback — confirmed real gap (`mcp.rs:2788-2791`).
- Cross-cutting claim "gap is index-time not query-time" — overstated. Query-path bugs (Issues 1-10) are confirmed wins too.

**`round-5-meta-robustness.md`**
- T13 failure detection gap — confirmed (`mcp.rs:2794`).
- T18 — same as above.

**`making-codesynapse-better.md`**
- Direction 32 (doctor command) — not implemented. All others: aspirational, no code bugs.

**`strategy-against-codegraph.md`**
- Pure strategic/positioning doc. No code-level claims to verify.

---

## How this index was built

This index was generated after 5 rounds of "anything else we can do to beat codegraph technically?" Each round captured the new directions in a dedicated doc. The diminishing returns analysis in each doc (`advanced-retrieval.md` Part 5, `round-5-meta-robustness.md` Part 5) tracks when planning is complete.

**Stop point:** Round 5. Each new direction adds <0.5-1 point. Execution > more planning.
