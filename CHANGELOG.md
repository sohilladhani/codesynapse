# Changelog

All notable changes to this project are documented here.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.1.0] - 2026-06-11

### Added

- **32 MCP tools** — hybrid BM25 + dense vector search over a structural knowledge graph
  - Context & search: `codesynapse_context`, `codesynapse_resolve`, `codesynapse_query_semantic`, `codesynapse_query_vector`, `codesynapse_query_graph`, `codesynapse_find_similar`, `codesynapse_find_usages`, `codesynapse_find_callers`
  - Navigation: `codesynapse_get_node`, `codesynapse_get_neighbors`, `codesynapse_outline`, `codesynapse_hierarchy`, `codesynapse_module_summary`, `codesynapse_read`, `codesynapse_read_method`, `codesynapse_read_with_callees`
  - Impact analysis: `codesynapse_blast_radius`, `codesynapse_blast_radius_multi`, `codesynapse_blast_radius_scored`, `codesynapse_diff`, `codesynapse_affected`
  - Graph analytics: `codesynapse_pagerank`, `codesynapse_god_nodes`, `codesynapse_detect_cycles`, `codesynapse_community_bridges`, `codesynapse_get_community`, `codesynapse_graph_stats`, `codesynapse_stats`
  - Path finding: `codesynapse_shortest_path`, `codesynapse_find_all_paths`, `codesynapse_weighted_path`
  - Build: `codesynapse_build`, `codesynapse_list_graphs`, `codesynapse_smart_summary`
- **20 language extractors** via tree-sitter: Python, TypeScript/JavaScript, Rust, Go, Java, C, C++, C#, Kotlin, Swift, PHP, Ruby, Scala, Dart, Lua, Zig, Haskell, Elixir, Julia, Groovy, SQL, Bash + more
- **Local CPU-only embeddings** via `potion-code-16M` (16M params, ~62MB, no GPU required, no API key)
- **Incremental indexing** — file-hash cache skips unchanged files on re-index
- **Watch mode** (`codesynapse watch`) — live graph updates on file save
- **Multi-module graph** — index multiple repos, query them as a unified global graph
- **MCP server** with `stdio` and `SSE` transport
- **Claude Code integration** — skill + MCP auto-config via `codesynapse setup --client claude`
- **Cursor integration** — rule + MCP auto-config via `codesynapse setup --client cursor`
- **No telemetry** — all processing is local; no data leaves your machine

[0.1.0]: https://github.com/sohilladhani/codesynapse/releases/tag/v0.1.0
