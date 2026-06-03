# MCP Tools Reference

32 tools exposed by the codesynapse MCP server over JSON-RPC (stdio). All tools accept a `graph` parameter (default: `"merged"`) unless noted otherwise.

---

## Graph search

### `codesynapse_query_vector`

Hybrid BM25 + dense vector search across the knowledge graph. Primary tool for "what handles / manages / processes X?" questions.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `query` | string | **required** | Natural language query |
| `graph` | string | `"merged"` | Graph module to search |
| `top_k` | integer | `8` | Number of results to return |

```
codesynapse_query_vector("manages authentication tokens")
codesynapse_query_vector("payment processing", graph="payments-service")
```

---

### `codesynapse_query_semantic`

Traverses `semantically_similar_to` edges from seed nodes. Finds functionally related nodes by meaning. Requires the graph to have been built with `--llm`; returns a graceful fallback if no semantic edges exist.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `query` | string | **required** | Natural language query |
| `graph` | string | `"merged"` | Graph module to search |
| `depth` | integer | `2` | Traversal depth |
| `min_confidence` | number | `0.7` | Minimum edge confidence threshold |

---

### `codesynapse_blast_radius`

Find all nodes reachable from a class within N hops. Shows what a change to this class could affect.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class to start BFS from |
| `graph` | string | `"merged"` | Graph module |
| `depth` | integer | `3` | BFS depth |

```
codesynapse_blast_radius("UserService")
codesynapse_blast_radius("PaymentProcessor", depth=2)
```

---

### `codesynapse_blast_radius_scored`

Blast radius with risk scores (0.0–1.0) per affected node. Risk factors: security/payment keywords, high in-degree, missing test coverage. Results sorted HIGH → MEDIUM → LOW.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class to start BFS from |
| `graph` | string | `"merged"` | Graph module |
| `depth` | integer | `3` | BFS depth |

---

### `codesynapse_blast_radius_multi`

Combined blast radius for multiple classes in one call. BFS from all seeds simultaneously; returns union of affected nodes grouped by hop distance.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_names` | string[] | **required** | Classes to start BFS from |
| `graph` | string | `"merged"` | Graph module |
| `depth` | integer | `3` | BFS depth |

```
codesynapse_blast_radius_multi(["AuthService", "TokenStore"])
```

---

### `codesynapse_hierarchy`

Show class inheritance tree: supertypes (what this class extends/implements) and implementors (what extends/implements this class).

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class to inspect |
| `graph` | string | `"merged"` | Graph module |

```
codesynapse_hierarchy("BaseRepository")
```

---

### `codesynapse_list_graphs`

List all registered graph modules with node and edge counts.

No parameters.

---

### `codesynapse_module_summary`

Node count, edge count, top god-nodes, and language breakdown for a specific module.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `module` | string | **required** | Module name (from `codesynapse_list_graphs`) |

---

### `codesynapse_build`

Reload the knowledge graph from disk. Call after `codesynapse module refresh` to pick up changes without restarting the MCP server.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `module` | string | `""` | Module to reload (empty = reload all) |

---

## Code reading

### `codesynapse_resolve`

One-call resolver: hybrid search → top-K seed nodes → reads outline + top method bodies. Returns full source context without any file reads. Primary tool for "how does X work?" questions.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `query` | string | **required** | Natural language query |
| `graph` | string | `"merged"` | Graph module to search |
| `top_k` | integer | `3` | Seed nodes to resolve |
| `max_chars` | integer | `8000` | Output character budget |

```
codesynapse_resolve("how does auth token validation work")
codesynapse_resolve("payment charge flow", top_k=5)
```

---

### `codesynapse_outline`

Get a compact structural outline of a class: methods, fields, line numbers. Use this before `codesynapse_read` to identify which lines to read.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class to outline |
| `graph` | string | `"merged"` | Graph module |

```
codesynapse_outline("UserRepository")
```

---

### `codesynapse_read`

Read specific lines from a class's source file, resolved via the knowledge graph. No need to know the file path.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class to read |
| `from_line` | integer | `1` | Start line (1-indexed) |
| `to_line` | integer | `0` | End line (0 = end of file) |
| `graph` | string | `"merged"` | Graph module |

```
codesynapse_read("UserRepository", from_line=45, to_line=80)
```

---

### `codesynapse_read_method`

Read a specific method body. Resolves class → source file → finds the method via brace tracking. No line numbers needed.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class containing the method |
| `method_name` | string | **required** | Method name |
| `graph` | string | `"merged"` | Graph module |

```
codesynapse_read_method("AuthService", "validateToken")
```

---

### `codesynapse_read_with_callees`

Read a method body and inline the bodies of same-class methods it calls. Gives full context without navigating call chains manually.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class containing the method |
| `method_name` | string | **required** | Method name |
| `depth` | integer | `1` | Callee inlining depth |
| `graph` | string | `"merged"` | Graph module |

---

## Navigation

### `codesynapse_find_callers`

Find all callers of a class or method via graph edges, with source text search as fallback.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class to find callers of |
| `method_name` | string | `""` | Method to find callers of (optional) |
| `graph` | string | `"merged"` | Graph module |

```
codesynapse_find_callers("PaymentService", "charge")
codesynapse_find_callers("UserRepository")
```

---

### `codesynapse_find_usages`

Find all source files that reference a class via imports, fields, parameters, or annotations.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `class_name` | string | **required** | Class to find usages of |
| `graph` | string | `"merged"` | Graph module |

---

## Graph analysis

### `codesynapse_query_graph`

Query the knowledge graph using natural language. Returns matching nodes with context.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `question` | string | **required** | Natural language question |
| `budget` | integer | `5` | Max nodes to return |

---

### `codesynapse_get_node`

Get a node by its full ID (`repo_tag::ClassName`).

| Parameter | Type | Default | Description |
|---|---|---|---|
| `node_id` | string | **required** | Full node ID |

---

### `codesynapse_get_neighbors`

Get neighbors of a node up to a given depth.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `node_id` | string | **required** | Full node ID |
| `depth` | integer | `1` | Traversal depth |
| `limit` | integer | `50` | Max neighbors to return |
| `offset` | integer | `0` | Pagination offset |

---

### `codesynapse_get_community`

Get all nodes in a community cluster by community ID.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `community_id` | integer | **required** | Community cluster ID |
| `limit` | integer | `50` | Max nodes to return |
| `offset` | integer | `0` | Pagination offset |

---

### `codesynapse_god_nodes`

Find the most connected nodes in the graph (highest degree). Useful for identifying architectural hotspots.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `top_n` | integer | `10` | Number of god nodes to return |

---

### `codesynapse_graph_stats`

Get overall graph statistics: node count, edge count, language breakdown, community count.

No parameters.

---

### `codesynapse_shortest_path`

Find the shortest path between two nodes using BFS.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `source` | string | **required** | Source node ID or label |
| `target` | string | **required** | Target node ID or label |

---

### `codesynapse_find_all_paths`

Find all paths between two nodes up to a maximum length.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `source` | string | **required** | Source node ID or label |
| `target` | string | **required** | Target node ID or label |
| `max_length` | integer | `5` | Maximum path length |

---

### `codesynapse_weighted_path`

Find the weighted shortest path using Dijkstra's algorithm.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `source` | string | **required** | Source node ID or label |
| `target` | string | **required** | Target node ID or label |
| `min_confidence` | number | `0.0` | Minimum edge confidence to traverse |

---

### `codesynapse_community_bridges`

Find bridge edges that connect different community clusters. Useful for identifying architectural coupling between modules.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `top_n` | integer | `10` | Number of bridge edges to return |

---

### `codesynapse_diff`

Compare the current graph against another graph file and return added/removed edges.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `other_graph` | string | **required** | Path to the other `graph.json` |

---

### `codesynapse_pagerank`

Compute PageRank scores for all nodes. Identifies architecturally important classes by link structure.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `top_n` | integer | `10` | Number of top-ranked nodes to return |
| `damping` | number | `0.85` | PageRank damping factor |
| `max_iter` | integer | `100` | Maximum iterations |

---

### `codesynapse_detect_cycles`

Detect cycles (strongly connected components) in the graph. Useful for finding circular dependencies.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `max_cycles` | integer | `10` | Maximum cycles to return |

---

### `codesynapse_smart_summary`

Generate a multi-level architectural summary of the graph.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `level` | string | `"architecture"` | `"detailed"` \| `"community"` \| `"architecture"` |
| `budget` | integer | `100` | Max nodes to include in summary |

---

### `codesynapse_find_similar`

Find structurally similar nodes based on graph topology. Requires Node2Vec embeddings.

| Parameter | Type | Default | Description |
|---|---|---|---|
| `node_id` | string | **required** | Node to find similar nodes for |
| `top_n` | integer | `10` | Number of similar nodes to return |

---

## Observability

### `codesynapse_stats`

Show codesynapse tool usage and estimated token savings across all sessions. Output contains Unicode box-drawing characters and bar charts — display verbatim.

No parameters.
