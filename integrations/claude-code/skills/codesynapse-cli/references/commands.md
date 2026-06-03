# Codesynapse CLI Command Reference

## Table of Contents
1. [query](#query) — Search the graph
2. [explain](#explain) — Node details and relationships
3. [affected](#affected) — Blast radius
4. [path](#path) — Shortest path
5. [stats](#stats) — Graph statistics
6. [analyze](#analyze) — God nodes and hub analysis
7. [cluster](#cluster) — Community detection
8. [diff](#diff) — Graph diff
9. [module](#module) — Manage modules
10. [build / extract](#build--extract) — Build the graph
11. [Troubleshooting](#troubleshooting)

---

## query

Search the graph using BM25 + optional dense search (hybrid RRF).

```bash
codesynapse query "<question>" --graph <path>
codesynapse query "<question>" --graph <path> --mode bfs --depth 2
codesynapse query "<question>" --graph <path> --mode dfs --depth 3
```

**Flags:**
- `--graph <path>` — Path to graph.json
- `--mode <bfs|dfs>` — Traversal mode (default: bfs)
- `--depth <N>` — Traversal depth (default: 2)

**Examples:**
```bash
codesynapse query "authentication flow" --graph ./codesynapse-out/graph.json
codesynapse query "UserService" --graph ./codesynapse-out/graph.json
codesynapse query "calls UserRepository" --graph ./codesynapse-out/graph.json
codesynapse query "references AuthMiddleware" --graph ./codesynapse-out/graph.json
```

---

## explain

Show a node's details and all its relationships.

```bash
codesynapse explain <id> --path <graph-path>
```

**Output includes:** file path, line number, community ID, all edges (calls, called-by, extends, implements, references).

**Examples:**
```bash
codesynapse explain UserService --path ./codesynapse-out/graph.json
codesynapse explain AuthController --path ./codesynapse-out/graph.json
```

---

## affected

Find all nodes affected by changes to a given node (blast radius).

```bash
codesynapse affected <id> --graph <path> --depth <N>
codesynapse affected <id> --graph <path> --depth 2 --relation calls
```

**Flags:**
- `--graph <path>` — Path to graph.json (OR `--path <path>`)
- `--depth <N>` — Search depth (default: 2)
- `--relation <type>` — Filter to specific relation type (can repeat)

**Examples:**
```bash
codesynapse affected UserService --graph ./codesynapse-out/graph.json --depth 2
codesynapse affected AuthMiddleware --graph ./codesynapse-out/graph.json --depth 3
codesynapse affected Database --graph ./codesynapse-out/graph.json --depth 1 --relation calls
```

---

## path

Find shortest path between two nodes.

```bash
codesynapse path <SOURCE> <TARGET>
```

**Examples:**
```bash
codesynapse path UserController UserRepository
codesynapse path AuthService TokenStore
```

---

## stats

Show graph statistics (node count, edge count, metrics).

```bash
codesynapse stats <path>
```

**Example:**
```bash
codesynapse stats ./codesynapse-out/graph.json
```

---

## analyze

Analyze the graph for god nodes and high-connectivity hubs.

```bash
codesynapse analyze <path>
```

**Example:**
```bash
codesynapse analyze ./codesynapse-out/graph.json
```

---

## cluster

Run community detection to group related nodes.

```bash
codesynapse cluster <path>
```

**Example:**
```bash
codesynapse cluster ./codesynapse-out/graph.json
```

---

## diff

Diff two graphs for structural equivalence.

```bash
codesynapse diff <path> --baseline <other-path>
```

**Example:**
```bash
codesynapse diff ./codesynapse-out/graph.json --baseline ./old-graph.json
```

---

## module

Manage source modules for global multi-repo graphs.

```bash
codesynapse module list                          # List all indexed modules
codesynapse module add <name> <source-path>      # Index a repo
codesynapse module refresh <name>                # Re-extract after code changes
codesynapse module remove <name>                 # Remove a module
```

**Examples:**
```bash
codesynapse module list
codesynapse module add myapp /home/user/projects/myapp
codesynapse module refresh myapp
```

---

## build / extract

Build a graph for a project without the module system.

```bash
codesynapse extract <source-path> -o fragments/
codesynapse build fragments/ -o codesynapse-out/graph.json
```

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| `command not found: codesynapse` | Try `/usr/local/bin/codesynapse` or `~/.local/bin/codesynapse`; ensure binary is on PATH |
| `--graph` path not found | Verify path; check `codesynapse module list` |
| Empty query results | Broaden query terms; try different phrasing |
| Stale graph after code changes | Run `codesynapse module refresh <name>` |
| `explain` node not found | Try class name without namespace prefix |
| `pagerank` not found | Use `codesynapse analyze <path>` instead |
