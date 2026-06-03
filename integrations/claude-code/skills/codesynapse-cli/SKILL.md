---
name: codesynapse-cli
description: |-
  Use this skill to answer codebase architecture questions, trace dependencies, find callers/callees, calculate blast radius, and explore the code graph.
  ACTIVATE when the user asks "how does X work", "what handles Y", "where is Z defined", "explain the mechanism", "who calls", "blast radius", "shortest path", "trace request flow", "inheritance hierarchy", "dependency graph", "call graph", "graph stats", "god nodes", or "architectural overview" — even if they don't mention codesynapse.
  Run `codesynapse query`, `codesynapse explain`, `codesynapse path`, `codesynapse affected`, and other CLI subcommands instead of reading files directly when a graph was built for this project.
  Do NOT activate for trivial file contents or simple syntax questions — use native Read/Grep for that.
---

# Codesynapse CLI Skill

Codesynapse has pre-indexed this codebase into a knowledge graph that knows about classes, functions, their relationships (calls, extends, implements), and source locations. The graph is more precise than grep for architecture questions.

MCP is not available in this environment. All codesynapse commands run via bash. Use the `codesynapse` binary — if `codesynapse` is not found, try `/usr/local/bin/codesynapse` then `~/.local/bin/codesynapse` (the two standard install locations). The project graph is at `./codesynapse-out/graph.json`. For global multi-module graphs, run `codesynapse module list` to see indexed modules.

**ALWAYS use codesynapse CLI commands before grep/find for architecture questions.**

## Quick Reference

| Intent | Command |
|--------|---------|
| General codebase query | `codesynapse query "<question>" --graph <path>` |
| Node details + relationships | `codesynapse explain <id> --path <graph-path>` |
| Shortest path A → B | `codesynapse path <A> <B>` |
| Blast radius of X | `codesynapse affected <X> --graph <path> --depth 2` |
| Graph statistics | `codesynapse stats <path>` |
| God/central nodes | `codesynapse analyze <path>` |
| Community detection | `codesynapse cluster <path>` |
| Module list | `codesynapse module list` |
| Graph diff | `codesynapse diff <path> --baseline <other>` |

## Tool Selection Priority

**STEP 1** — Architecture questions ("how does X work", "what handles Y", "where is Z defined"):
```bash
codesynapse query "<question>" --graph ./codesynapse-out/graph.json
```
Parse the output for class/function names, file paths, relationships. Top results are most relevant.

**STEP 2** — Need details on a specific node found in Step 1:
```bash
codesynapse explain <ClassName> --path ./codesynapse-out/graph.json
```
Shows callers, callees, extends, implements, file path, and line number.

**STEP 3** — Blast radius / change impact:
```bash
codesynapse affected <ClassName> --graph ./codesynapse-out/graph.json --depth 2
```
Use `--depth 1` for direct dependencies, `--depth 3` for wider impact. Filter by `--relation calls` if needed.

**STEP 4** — Dependency tracing / request flow:
```bash
codesynapse path <FromClass> <ToClass>
```
Then run `codesynapse explain <intermediate>` for each node in the chain.

**STEP 5** — Structural ownership ("what module owns X"):
```bash
codesynapse query "<concept>" --graph ./codesynapse-out/graph.json
```
Look at file paths in results to determine module ownership.

## Approximating Full Context (no MCP `codesynapse_context`)

The MCP tool `codesynapse_context` returns source bodies + 1-hop call graph. Approximate it with this chain:

```bash
# 1. Find the exact node
codesynapse query "ClassName" --graph ./codesynapse-out/graph.json

# 2. Get details and neighbors
codesynapse explain ClassName --path ./codesynapse-out/graph.json

# 3. Get 1-hop call graph
codesynapse affected ClassName --graph ./codesynapse-out/graph.json --depth 1 --relation calls

# 4. Read source file at the line number from explain output
# Use the native Read tool with file path and line from explain
```

## Behavioral Rules

- Do NOT grep/find to verify graph results. The graph is the source of truth.
- If a query returns no results, broaden the query terms and try again.
- If the graph is stale after code changes: `codesynapse module refresh <name>` or rebuild.
- To read method bodies: use the file path and line number from `explain` output, then use the native Read tool.
- Chain commands when one isn't enough: `query` → `explain` → `read`.
- If `codesynapse module list` fails, fall back to `--graph ./codesynapse-out/graph.json`.

## Output Parsing Guide

- **`query`**: Ranked list of matching nodes with relevance scores, file paths, line numbers. Top = most relevant.
- **`explain`**: Node label, ID, file path, line number, community ID, and all relationship edges (calls, called-by, extends, implements).
- **`affected`**: Seed node + all affected nodes grouped by distance, with file paths.
- **`path`**: Chain of nodes with edge types between them (A →[calls]→ B →[extends]→ C).
- **`analyze`**: God nodes (high connectivity) and surprising connections.
- **`stats`**: Node count, edge count, graph metrics.
- **`cluster`**: Community groupings with node memberships.

For detailed command syntax and flags, see `references/commands.md` or run `codesynapse <subcommand> --help`.
