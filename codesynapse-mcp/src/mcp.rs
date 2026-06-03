use codesynapse_core::analyze::Analyzer;
use codesynapse_core::embedding::StaticEmbedder;
use codesynapse_core::error::Result;
use codesynapse_core::global_graph::global_list;
use codesynapse_core::graph::{GraphStore, SledGraphStore, StoreBackend};
use codesynapse_core::types::{Edge, Node};
use codesynapse_serve::context::context_query;
use codesynapse_serve::graph_query::{
    load_graph, query_graph_text_hybrid, query_top_nodes, ServeEdge, ServeGraph, ServeNode,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

// ---------------------------------------------------------------------------
// Server instructions — injected into agent system prompt via MCP initialize
// ---------------------------------------------------------------------------

pub const CODESYNAPSE_INSTRUCTIONS: &str = "\
Use Codesynapse MCP tools before reading files or answering architecture questions.\n\
NEVER use grep, find, Bash, or subagents to answer 'how does X work' questions.\n\
Subagents do not inherit MCP — they fall back to grep. Always use tools directly.\n\
\n\
Tool selection — follow this hierarchy exactly:\n\
\n\
STEP 1 — ALWAYS call codesynapse_context FIRST for ANY of these:\n\
  'How does X work?', 'What handles Y?', 'Where is Z?', 'Explain the mechanism for...'\n\
  Natural language OR symbol name both work.\n\
  If result does NOT start with '[No exact match' → review before answering:\n\
    For 'what handles / what builds / what processes / what manages' questions:\n\
      Check that results contain a HANDLER or BUILDER (a function/class that catches,\n\
      dispatches, or constructs the thing) — not just code that raises or invokes it.\n\
      If results only show WHERE something is raised/called (e.g. views raising NotFound)\n\
      but not WHO handles it:\n\
        1. Identify the exception/concept class name from the results (e.g. NotFound, Http404)\n\
        2. Call codesynapse_resolve(\"<ClassName> handler\") — this surfaces registered\n\
           callbacks and entry points that string-based config (Django settings, Spring\n\
           @Bean, Express app.use) wires up invisibly to the graph.\n\
        Do NOT fall back to grep — codesynapse_resolve finds entry points grep cannot.\n\
    Otherwise → answer directly from it.\n\
\n\
STEP 2 — If codesynapse_context starts with '[No exact match — showing semantic results]':\n\
  Identify specific type/function names in the results (CamelCase classes, snake_case functions).\n\
  Call codesynapse_context(\"ExactName\") with EACH name — exact match returns callers+callees.\n\
  For request flow questions this surfaces ALL callers of a service in one call.\n\
  If no names found → call codesynapse_resolve(\"question\") for deeper hybrid search.\n\
\n\
STEP 2b — If exploring a multi-component mechanism (scheduler, I/O pipeline, runtime):\n\
  Check results for TWO warning signs — either one means you have ONE layer, not architecture:\n\
    WARNING A: All results are from the SAME directory or file.\n\
    WARNING B: All results are methods/functions — no struct, enum, impl, or module entries.\n\
  If either warning fires after 2-3 calls, DO NOT answer yet. Zoom out first:\n\
    1. codesynapse_list_graphs()                   — identify the right module name\n\
    2. codesynapse_module_summary(\"module\")         — get top-level architectural map\n\
    3. Find the main struct/enum in the summary (e.g. Runtime, Scheduler, Worker)\n\
    4. codesynapse_context(\"StructName\")            — exact match returns fields + callers\n\
    5. codesynapse_read_with_callees(\"Struct\", \"method\") — gets method body + everything it calls in one shot\n\
       This surfaces adjacent methods (LIFO slot, batching, dynamic tuning) without knowing their names upfront.\n\
       Fall back to codesynapse_read_method only for small leaf methods.\n\
  Use codesynapse_read_method(\"TypeName\", \"methodName\") to get COMPLETE method bodies\n\
  when context returns a partial snippet and you need full accuracy.\n\
\n\
STEP 3 — codesynapse_query_vector: ONLY for structural ownership questions:\n\
  'Which module owns X?', 'What class manages Y?' — NOT for mechanism questions.\n\
  Do NOT grep/find to verify graph results — trust the graph.\n\
\n\
Other tools (use only when specifically needed):\n\
  About to change a class               → codesynapse_blast_radius(\"ClassName\")\n\
  Multiple classes changing (PR review) → codesynapse_blast_radius_multi([\"A\",\"B\",\"C\"])\n\
  Blast radius + risk scores            → codesynapse_blast_radius_scored(\"ClassName\")\n\
  Class inheritance tree                → codesynapse_hierarchy(\"ClassName\")\n\
  Overview of a module                  → codesynapse_list_graphs() then codesynapse_module_summary(\"name\")\n\
  Read/edit a class                     → codesynapse_outline(\"ClassName\") then codesynapse_read(...)\n\
  Know exact method to read             → codesynapse_read_method(\"ClassName\", \"methodName\")\n\
  Need method + what it calls           → codesynapse_read_with_callees(\"ClassName\", \"methodName\")\n\
  Who calls X.method?                   → codesynapse_find_callers(\"ClassName\", \"methodName\")\n\
  Request flow trace (controller→svc→DB) → context(\"ExactServiceClass\") — shows callers+callees\n\
  All files referencing a class         → codesynapse_find_usages(\"ClassName\")\n\
  Graph stale after module refresh      → codesynapse_build()\n\
  Session usage + token savings         → codesynapse_stats()\n\
\n\
Graph selection:\n\
  Default graph is \"merged\" (all modules combined). Use a module-specific graph\n\
  when merged returns too many results or unrelated modules bleed in (>500 nodes):\n\
    codesynapse_list_graphs()                          — see all modules with node counts\n\
    codesynapse_query_vector(\"X\", graph=\"mymodule\")   — scope to one module\n\
    codesynapse_module_summary(\"mymodule\")             — overview before diving in\n\
  If results keep returning the wrong module → remove it:\n\
    codesynapse module remove <name>\n\
\n\
Add a new module:\n\
  1. In terminal: codesynapse module add <name> /absolute/path\n\
  2. Back here:   codesynapse_build()";

// ---------------------------------------------------------------------------
// Tool definition
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "codesynapse_query_graph".into(),
            description: "Query the knowledge graph using natural language".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "budget": {"type": "integer", "default": 5}
                },
                "required": ["question"]
            }),
        },
        ToolDef {
            name: "codesynapse_get_node".into(),
            description: "Get a node by its ID".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"node_id": {"type": "string"}},
                "required": ["node_id"]
            }),
        },
        ToolDef {
            name: "codesynapse_get_neighbors".into(),
            description: "Get neighbors of a node up to a given depth".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": {"type": "string"},
                    "depth": {"type": "integer", "default": 1},
                    "limit": {"type": "integer", "default": 50},
                    "offset": {"type": "integer", "default": 0}
                },
                "required": ["node_id"]
            }),
        },
        ToolDef {
            name: "codesynapse_get_community".into(),
            description: "Get nodes in a community by ID".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "community_id": {"type": "integer"},
                    "limit": {"type": "integer", "default": 50},
                    "offset": {"type": "integer", "default": 0}
                },
                "required": ["community_id"]
            }),
        },
        ToolDef {
            name: "codesynapse_god_nodes".into(),
            description: "Find the most connected (god) nodes".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"top_n": {"type": "integer", "default": 10}},
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_graph_stats".into(),
            description: "Get overall graph statistics".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_shortest_path".into(),
            description: "Find the shortest path between two nodes (BFS)".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                    "target": {"type": "string"}
                },
                "required": ["source", "target"]
            }),
        },
        ToolDef {
            name: "codesynapse_find_all_paths".into(),
            description: "Find all paths between two nodes up to a max length".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                    "target": {"type": "string"},
                    "max_length": {"type": "integer", "default": 5}
                },
                "required": ["source", "target"]
            }),
        },
        ToolDef {
            name: "codesynapse_weighted_path".into(),
            description: "Find the weighted shortest path using Dijkstra".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                    "target": {"type": "string"},
                    "min_confidence": {"type": "number", "default": 0.0}
                },
                "required": ["source", "target"]
            }),
        },
        ToolDef {
            name: "codesynapse_community_bridges".into(),
            description: "Find bridge edges that connect communities".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"top_n": {"type": "integer", "default": 10}},
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_diff".into(),
            description: "Compare two graphs and find added/removed edges".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"other_graph": {"type": "string"}},
                "required": ["other_graph"]
            }),
        },
        ToolDef {
            name: "codesynapse_pagerank".into(),
            description: "Compute PageRank scores for all nodes".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "top_n": {"type": "integer", "default": 10},
                    "damping": {"type": "number", "default": 0.85},
                    "max_iter": {"type": "integer", "default": 100}
                },
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_detect_cycles".into(),
            description: "Detect cycles (strongly connected components) in the graph".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"max_cycles": {"type": "integer", "default": 10}},
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_smart_summary".into(),
            description: "Generate a multi-level summary of the graph".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "level": {"type": "string", "enum": ["detailed", "community", "architecture"], "default": "architecture"},
                    "budget": {"type": "integer", "default": 100}
                },
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_find_similar".into(),
            description: "Find structurally similar nodes (requires Node2Vec)".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "top_n": {"type": "integer", "default": 10},
                    "node_id": {"type": "string"}
                },
                "required": ["node_id"]
            }),
        },
        ToolDef {
            name: "codesynapse_query_vector".into(),
            description: "Structural ownership search — use ONLY when the question is 'what class/module handles X?' or 'what manages Y?' where you need to find which component owns a concept. NOT for mechanism questions ('how does X work') — use codesynapse_context for those. Returns a ranked node list, no source bodies.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"},
                    "top_k": {"type": "integer", "default": 8}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "codesynapse_blast_radius".into(),
            description: "Find all nodes reachable from a class within N hops".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"},
                    "depth": {"type": "integer", "default": 3}
                },
                "required": ["class_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_blast_radius_scored".into(),
            description: "Blast radius of a class with risk scores (0.0–1.0) per affected node. Risk factors: security/payment keywords, high in-degree, no test coverage. Sorted HIGH→MEDIUM→LOW.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"},
                    "depth": {"type": "integer", "default": 3}
                },
                "required": ["class_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_blast_radius_multi".into(),
            description: "Combined blast radius for multiple classes in one call. BFS from all seeds simultaneously, union of affected nodes grouped by hop distance.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_names": {"type": "array", "items": {"type": "string"}},
                    "graph": {"type": "string", "default": "merged"},
                    "depth": {"type": "integer", "default": 3}
                },
                "required": ["class_names"]
            }),
        },
        ToolDef {
            name: "codesynapse_query_semantic".into(),
            description: "Traverse semantically_similar_to edges from seed nodes. Finds functionally related nodes by meaning. Requires graph built with --llm flag; returns graceful fallback if no semantic edges exist.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"},
                    "depth": {"type": "integer", "default": 2},
                    "min_confidence": {"type": "number", "default": 0.7}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "codesynapse_hierarchy".into(),
            description: "Show class inheritance tree (supertypes and implementors)".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"}
                },
                "required": ["class_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_list_graphs".into(),
            description: "List all registered graph modules with node/edge counts".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_module_summary".into(),
            description: "Node count, edge count, top god-nodes and language breakdown for a module".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "module": {"type": "string"}
                },
                "required": ["module"]
            }),
        },
        ToolDef {
            name: "codesynapse_outline".into(),
            description: "Get compact structural outline of a class (methods, fields, line numbers) via the knowledge graph".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"}
                },
                "required": ["class_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_read".into(),
            description: "Read specific lines from a class's source file resolved via the knowledge graph".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "from_line": {"type": "integer", "default": 1},
                    "to_line": {"type": "integer", "default": 0},
                    "graph": {"type": "string", "default": "merged"}
                },
                "required": ["class_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_read_method".into(),
            description: "Read a specific method body — resolves class → file → finds method via brace tracking".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "method_name": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"}
                },
                "required": ["class_name", "method_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_read_with_callees".into(),
            description: "Read a method AND inline bodies of same-class methods it calls".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "method_name": {"type": "string"},
                    "depth": {"type": "integer", "default": 1},
                    "graph": {"type": "string", "default": "merged"}
                },
                "required": ["class_name", "method_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_find_callers".into(),
            description: "Find all callers of a class or method via graph edges, then source text search fallback".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "method_name": {"type": "string", "default": ""},
                    "graph": {"type": "string", "default": "merged"}
                },
                "required": ["class_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_find_usages".into(),
            description: "Find all source files that reference a class (imports, fields, parameters, annotations)".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "class_name": {"type": "string"},
                    "graph": {"type": "string", "default": "merged"}
                },
                "required": ["class_name"]
            }),
        },
        ToolDef {
            name: "codesynapse_context".into(),
            description: concat!(
                "PRIMARY tool — call this FIRST for ALL mechanism, architecture, and 'how does X work' questions. ",
                "Accepts natural language or a symbol name. ",
                "Finds entry points via symbol matching, expands one hop via call graph edges, ",
                "and returns full source bodies with line numbers. ",
                "Falls back to semantic search when no exact symbol match is found. ",
                "Answer directly from this output — do NOT call resolve or query_vector afterward. ",
                "For request flow questions: if result is semantic fallback, call context(\"ExactClassName\") ",
                "with specific class names found in results — exact name triggers call-graph expansion, ",
                "showing all callers (e.g. every controller that uses a service) and callees in one shot."
            ).into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query":     {"type": "string", "description": "Natural-language question or symbol name — accepts both"},
                    "graph":     {"type": "string", "default": "merged"},
                    "max_chars": {"type": "integer", "default": 16000, "description": "Response size cap"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "codesynapse_resolve".into(),
            description: concat!(
                "FALLBACK ONLY — use only when codesynapse_context returns empty or no results. ",
                "Runs hybrid BM25+dense search and returns outlines + top method bodies. ",
                "Do NOT use as the first tool for 'how does X work' questions — use codesynapse_context first."
            ).into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query":     {"type": "string", "description": "Natural-language question or concept"},
                    "graph":     {"type": "string", "default": "merged"},
                    "top_k":     {"type": "integer", "default": 5, "description": "Max classes to include"},
                    "max_chars": {"type": "integer", "default": 24000, "description": "Response size cap"}
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "codesynapse_build".into(),
            description: "Reload the knowledge graph from disk — call after bootstrap to pick up changes without MCP restart".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "module": {"type": "string", "default": ""}
                },
                "required": []
            }),
        },
        ToolDef {
            name: "codesynapse_stats".into(),
            description: "Show codesynapse tool usage and estimated token savings across all sessions".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Outline helpers (Phase 3a)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct OutlineItem {
    kind: String,
    name: String,
    line: usize, // 1-indexed
}

fn outline_items(path: &Path, content: &str) -> Vec<OutlineItem> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "py" => outline_python(content),
        "js" | "ts" | "tsx" | "jsx" => outline_js(content),
        "rs" => outline_rust(content),
        "go" => outline_go(content),
        "rb" => outline_ruby(content),
        "ex" | "exs" => outline_elixir(content),
        _ => outline_java(content),
    }
}

fn outline_java(content: &str) -> Vec<OutlineItem> {
    let class_re = Regex::new(
        r"(?i)^\s*(?:(?:public|protected|private|abstract|final|static)\s+)*(class|interface|enum|record)\s+(\w+)"
    ).unwrap();
    let method_re = Regex::new(
        r"^\s*(?:(?:public|protected|private|static|final|abstract|synchronized|native|default|override)\s+)+(?:[\w<>\[\],\s]+\s+)(\w+)\s*\("
    ).unwrap();
    let field_re = Regex::new(
        r"^\s*(?:(?:public|protected|private|static|final|volatile|transient)\s+)+([\w<>\[\]]+)\s+(\w+)\s*[=;]"
    ).unwrap();
    let mut items = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let lineno = i + 1;
        if let Some(cap) = class_re.captures(line) {
            items.push(OutlineItem {
                kind: cap[1].to_lowercase(),
                name: cap[2].to_string(),
                line: lineno,
            });
        } else if let Some(cap) = method_re.captures(line) {
            let name = cap[1].to_string();
            if !line.split('(').next().is_some_and(|s| s.contains('=')) {
                items.push(OutlineItem {
                    kind: "method".into(),
                    name,
                    line: lineno,
                });
            }
        } else if let Some(cap) = field_re.captures(line) {
            items.push(OutlineItem {
                kind: "field".into(),
                name: cap[2].to_string(),
                line: lineno,
            });
        }
    }
    items
}

fn outline_python(content: &str) -> Vec<OutlineItem> {
    let class_re = Regex::new(r"^class\s+(\w+)").unwrap();
    let method_re = Regex::new(r"^(\s+)def\s+(\w+)\s*\(").unwrap();
    let func_re = Regex::new(r"^def\s+(\w+)\s*\(").unwrap();
    let mut items = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let lineno = i + 1;
        if let Some(cap) = class_re.captures(line) {
            items.push(OutlineItem {
                kind: "class".into(),
                name: cap[1].into(),
                line: lineno,
            });
        } else if let Some(cap) = method_re.captures(line) {
            items.push(OutlineItem {
                kind: "method".into(),
                name: cap[2].into(),
                line: lineno,
            });
        } else if let Some(cap) = func_re.captures(line) {
            items.push(OutlineItem {
                kind: "method".into(),
                name: cap[1].into(),
                line: lineno,
            });
        }
    }
    items
}

fn outline_js(content: &str) -> Vec<OutlineItem> {
    const KEYWORDS: &[&str] = &[
        "if",
        "for",
        "while",
        "switch",
        "catch",
        "return",
        "const",
        "let",
        "var",
        "import",
        "export",
        "new",
        "throw",
        "case",
        "default",
        "else",
        "try",
        "do",
        "typeof",
        "instanceof",
        "function",
    ];
    let class_re = Regex::new(
        r"^\s*(?:export\s+(?:default\s+)?)?(?:abstract\s+)?(class|interface|type|enum)\s+(\w+)",
    )
    .unwrap();
    let method_re = Regex::new(r"^\s*(?:(?:async|static|get|set)\s+)*(\w+)\s*\(").unwrap();
    let field_re =
        Regex::new(r"^\s*(?:(?:public|private|protected|readonly|static)\s+)+(\w+)\s*[?!:=]")
            .unwrap();
    let mut items = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let lineno = i + 1;
        let stripped = line.trim();
        if stripped.starts_with("//") || stripped.starts_with('*') {
            continue;
        }
        if let Some(cap) = class_re.captures(line) {
            items.push(OutlineItem {
                kind: cap[1].to_lowercase(),
                name: cap[2].into(),
                line: lineno,
            });
        } else if line.contains('(') {
            if let Some(cap) = method_re.captures(line) {
                let name = cap[1].to_string();
                if !KEYWORDS.contains(&name.as_str()) {
                    items.push(OutlineItem {
                        kind: "method".into(),
                        name,
                        line: lineno,
                    });
                }
            }
        } else if let Some(cap) = field_re.captures(line) {
            let name = cap[1].to_string();
            if !KEYWORDS.contains(&name.as_str()) {
                items.push(OutlineItem {
                    kind: "field".into(),
                    name,
                    line: lineno,
                });
            }
        }
    }
    items
}

fn outline_rust(content: &str) -> Vec<OutlineItem> {
    let type_re =
        Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|type)\s+(\w+)").unwrap();
    // impl Trait for Type  →  capture Type; impl Type  →  capture Type
    let impl_for_re = Regex::new(r"^\s*impl[^{]*\bfor\s+(\w+)").unwrap();
    let impl_re = Regex::new(r"^\s*impl(?:\s*<[^>]*>)?\s+(\w+)").unwrap();
    let fn_re = Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)").unwrap();

    let mut items = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let lineno = i + 1;
        let stripped = line.trim();
        if stripped.starts_with("//") || stripped.starts_with('*') || stripped.starts_with('#') {
            continue;
        }
        if let Some(cap) = type_re.captures(line) {
            let kind = if line.contains("struct") {
                "class"
            } else if line.contains("trait") {
                "interface"
            } else {
                "enum"
            };
            items.push(OutlineItem {
                kind: kind.into(),
                name: cap[1].into(),
                line: lineno,
            });
        } else if let Some(cap) = impl_for_re.captures(line) {
            items.push(OutlineItem {
                kind: "class".into(),
                name: cap[1].into(),
                line: lineno,
            });
        } else if let Some(cap) = impl_re.captures(line) {
            items.push(OutlineItem {
                kind: "class".into(),
                name: cap[1].into(),
                line: lineno,
            });
        } else if let Some(cap) = fn_re.captures(line) {
            items.push(OutlineItem {
                kind: "method".into(),
                name: cap[1].into(),
                line: lineno,
            });
        }
    }
    items
}

fn outline_go(content: &str) -> Vec<OutlineItem> {
    let type_re = Regex::new(r"^\s*type\s+(\w+)\s+(?:struct|interface)").unwrap();
    let fn_re = Regex::new(r"^\s*func\s+(?:\([^)]*\)\s+)?(\w+)\s*\(").unwrap();

    let mut items = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let lineno = i + 1;
        let stripped = line.trim();
        if stripped.starts_with("//") {
            continue;
        }
        if let Some(cap) = type_re.captures(line) {
            let kind = if line.contains("interface") {
                "interface"
            } else {
                "class"
            };
            items.push(OutlineItem {
                kind: kind.into(),
                name: cap[1].into(),
                line: lineno,
            });
        } else if let Some(cap) = fn_re.captures(line) {
            items.push(OutlineItem {
                kind: "method".into(),
                name: cap[1].into(),
                line: lineno,
            });
        }
    }
    items
}

fn outline_ruby(content: &str) -> Vec<OutlineItem> {
    let class_re = Regex::new(r"^\s*(?:class|module)\s+(\w+)").unwrap();
    let method_re = Regex::new(r"^\s*def\s+(self\.)?(\w+)").unwrap();

    let mut items = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let lineno = i + 1;
        let stripped = line.trim();
        if stripped.starts_with('#') {
            continue;
        }
        if let Some(cap) = class_re.captures(line) {
            items.push(OutlineItem {
                kind: "class".into(),
                name: cap[1].into(),
                line: lineno,
            });
        } else if let Some(cap) = method_re.captures(line) {
            items.push(OutlineItem {
                kind: "method".into(),
                name: cap[2].into(),
                line: lineno,
            });
        }
    }
    items
}

fn outline_elixir(content: &str) -> Vec<OutlineItem> {
    let module_re = Regex::new(r"^\s*defmodule\s+([\w.]+)").unwrap();
    let fn_re = Regex::new(r"^\s*(?:def|defp|defmacro|defmacrop)\s+(\w+)").unwrap();

    let mut items = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let lineno = i + 1;
        let stripped = line.trim();
        if stripped.starts_with('#') {
            continue;
        }
        if let Some(cap) = module_re.captures(line) {
            items.push(OutlineItem {
                kind: "class".into(),
                name: cap[1].into(),
                line: lineno,
            });
        } else if let Some(cap) = fn_re.captures(line) {
            items.push(OutlineItem {
                kind: "method".into(),
                name: cap[1].into(),
                line: lineno,
            });
        }
    }
    items
}

fn detect_method_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0i32;
    let mut opened = false;
    for (i, line) in lines[start..].iter().enumerate() {
        depth += line.chars().filter(|&c| c == '{').count() as i32;
        depth -= line.chars().filter(|&c| c == '}').count() as i32;
        if depth > 0 {
            opened = true;
        }
        if opened && depth <= 0 {
            return start + i;
        }
    }
    lines.len().saturating_sub(1)
}

fn extract_method_range(
    lines: &[&str],
    outline: &[OutlineItem],
    method_name: &str,
) -> Option<(usize, usize)> {
    let lower = method_name.to_lowercase();
    for item in outline {
        if matches!(item.kind.as_str(), "method" | "function")
            && item.name.to_lowercase().starts_with(&lower)
        {
            let start_0 = item.line - 1;
            let end_0 = detect_method_end(lines, start_0);
            return Some((item.line, end_0 + 1));
        }
    }
    None
}

fn source_roots_from_graph(g: &ServeGraph) -> Vec<PathBuf> {
    let paths: Vec<PathBuf> = g
        .nodes_iter()
        .map(|(_, n)| PathBuf::from(&n.source_file))
        .filter(|p| p.is_absolute())
        .collect();
    if paths.is_empty() {
        return Vec::new();
    }
    let mut common = paths[0].clone();
    for p in &paths[1..] {
        while !p.starts_with(&common) {
            common = match common.parent() {
                Some(par) => par.to_path_buf(),
                None => return vec![PathBuf::from("/")],
            };
        }
    }
    if common.as_os_str().is_empty() {
        return Vec::new();
    }
    vec![common]
}

fn find_nodes_by_label<'a>(g: &'a ServeGraph, name: &str) -> Vec<(&'a str, &'a ServeNode)> {
    let lower = name.to_lowercase();
    let mut exact: Vec<(&str, &ServeNode)> = Vec::new();
    let mut partial: Vec<(&str, &ServeNode)> = Vec::new();
    for (id, node) in g.nodes_iter() {
        let nl = node.label.to_lowercase();
        if nl == lower {
            exact.push((id, node));
        } else if nl.contains(&lower) {
            partial.push((id, node));
        }
    }
    partial.sort_by_key(|(_, n)| n.label.len());
    exact.extend(partial);
    exact
}

fn search_source_files(
    roots: &[PathBuf],
    exts: &[&str],
    pattern: &Regex,
    max_hits: usize,
) -> Vec<(String, usize, String)> {
    let mut hits = Vec::new();
    'outer: for root in roots {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let ext = entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if !exts.contains(&ext) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let rel = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .to_string();
                for (lineno, line) in content.lines().enumerate() {
                    if pattern.is_match(line) {
                        let snippet: String = line.trim().chars().take(120).collect();
                        hits.push((rel.clone(), lineno + 1, snippet));
                        if hits.len() >= max_hits {
                            break 'outer;
                        }
                    }
                }
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

type SourceCacheMap = HashMap<PathBuf, (SystemTime, String, Vec<OutlineItem>)>;

pub struct McpServer {
    backend: Mutex<StoreBackend>,
    graph_path: Option<PathBuf>,
    last_mtime: Mutex<Option<SystemTime>>,
    global_dir: PathBuf,
    embedder: Option<StaticEmbedder>,
    node_embeddings: HashMap<String, Vec<f32>>,
    graph_cache: Mutex<HashMap<String, (SystemTime, Arc<ServeGraph>)>>,
    pub(crate) source_cache: Mutex<SourceCacheMap>,
    stale_check: Mutex<Option<bool>>,
    telemetry: Arc<codesynapse_core::telemetry::Telemetry>,
}

impl McpServer {
    pub fn new(graph_path: &Path) -> Result<Self> {
        let global_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".codesynapse");
        Self::new_with_global(graph_path, global_dir)
    }

    pub fn new_with_global(graph_path: &Path, global_dir: PathBuf) -> Result<Self> {
        let store = SledGraphStore::open(graph_path)?;
        let backend = StoreBackend::Sled(store);

        let model_path = global_dir.join("models").join("potion-code-16M");
        let (embedder, node_embeddings) = if model_path.exists() {
            let emb_path = global_dir.join("embeddings.json");
            if emb_path.exists() {
                let embedder = StaticEmbedder::from_path(&model_path).ok();
                let embs: HashMap<String, Vec<f32>> = std::fs::read_to_string(&emb_path)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_default();
                (embedder, embs)
            } else {
                (None, HashMap::new())
            }
        } else {
            (None, HashMap::new())
        };

        let telemetry = Arc::new(codesynapse_core::telemetry::Telemetry::new(
            global_dir.clone(),
        ));
        Ok(McpServer {
            backend: Mutex::new(backend),
            graph_path: Some(graph_path.to_path_buf()),
            last_mtime: Mutex::new(None),
            global_dir,
            embedder,
            node_embeddings,
            graph_cache: Mutex::new(HashMap::new()),
            source_cache: Mutex::new(HashMap::new()),
            stale_check: Mutex::new(None),
            telemetry,
        })
    }

    fn maybe_reload(&self) {
        let path = match &self.graph_path {
            Some(p) => p,
            None => return,
        };
        let mtime = match std::fs::metadata(path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return,
        };
        let mut last = self.last_mtime.lock().unwrap();
        if last.is_none_or(|prev| prev != mtime) {
            if let Ok(store) = SledGraphStore::open(path) {
                *self.backend.lock().unwrap() = StoreBackend::Sled(store);
            }
            *last = Some(mtime);
        }
    }

    pub fn run(&self) -> Result<()> {
        let stdin = io::stdin();
        let stdout = io::stdout();
        self.run_on(stdin.lock(), stdout.lock())
    }

    pub fn run_on<R: BufRead, W: Write>(&self, reader: R, mut writer: W) -> Result<()> {
        self.telemetry.flush_bg();
        for line in reader.lines() {
            let line =
                line.map_err(|e| codesynapse_core::error::CodeSynapseError::msg(e.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    let resp = JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: None,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32700,
                            message: format!("Parse error: {}", e),
                            data: None,
                        }),
                    };
                    let mut out = serde_json::to_string(&resp)?;
                    out.push('\n');
                    writer.write_all(out.as_bytes())?;
                    writer.flush()?;
                    continue;
                }
            };

            self.maybe_reload();
            let response = self.handle_request(&request);
            let mut out = serde_json::to_string(&response)?;
            out.push('\n');
            writer.write_all(out.as_bytes())?;
            writer.flush()?;
        }

        self.telemetry.persist_sync();
        Ok(())
    }

    fn handle_request(&self, request: &JsonRpcRequest) -> JsonRpcResponse {
        if request.method == "notifications/initialized" {
            return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: None,
                result: None,
                error: None,
            };
        }

        let result = match request.method.as_str() {
            "initialize" => Ok(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "codesynapse", "version": env!("CARGO_PKG_VERSION") },
                "instructions": CODESYNAPSE_INSTRUCTIONS
            })),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(request),
            _ => Err(format!("Unknown method: {}", request.method)),
        };

        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id.clone(),
                result: Some(value),
                error: None,
            },
            Err(msg) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: msg,
                    data: None,
                }),
            },
        }
    }

    fn handle_tools_list(&self) -> std::result::Result<Value, String> {
        let tools: Vec<Value> = tool_defs()
            .into_iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                })
            })
            .collect();
        Ok(serde_json::json!({ "tools": tools }))
    }

    fn handle_tools_call(&self, request: &JsonRpcRequest) -> std::result::Result<Value, String> {
        let params = request
            .params
            .as_ref()
            .and_then(|p| p.as_object())
            .ok_or_else(|| "Missing params object".to_string())?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing tool name".to_string())?;

        let arguments = params
            .get("arguments")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        let text = match name {
            "codesynapse_query_graph" => self.tool_query_graph(&arguments),
            "codesynapse_get_node" => self.tool_get_node(&arguments),
            "codesynapse_get_neighbors" => self.tool_get_neighbors(&arguments),
            "codesynapse_get_community" => self.tool_get_community(&arguments),
            "codesynapse_god_nodes" => self.tool_god_nodes(&arguments),
            "codesynapse_graph_stats" => self.tool_graph_stats(&arguments),
            "codesynapse_shortest_path" => self.tool_shortest_path(&arguments),
            "codesynapse_find_all_paths" => self.tool_find_all_paths(&arguments),
            "codesynapse_weighted_path" => self.tool_weighted_path(&arguments),
            "codesynapse_community_bridges" => self.tool_community_bridges(&arguments),
            "codesynapse_diff" => self.tool_graph_diff(&arguments),
            "codesynapse_pagerank" => self.tool_pagerank(&arguments),
            "codesynapse_detect_cycles" => self.tool_detect_cycles(&arguments),
            "codesynapse_smart_summary" => self.tool_smart_summary(&arguments),
            "codesynapse_find_similar" => self.tool_find_similar(&arguments),
            "codesynapse_query_vector" => self.tool_graph_query_vector(&arguments),
            "codesynapse_blast_radius" => self.tool_graph_blast_radius(&arguments),
            "codesynapse_blast_radius_scored" => self.tool_graph_blast_radius_scored(&arguments),
            "codesynapse_blast_radius_multi" => self.tool_graph_blast_radius_multi(&arguments),
            "codesynapse_query_semantic" => self.tool_graph_query_semantic(&arguments),
            "codesynapse_hierarchy" => self.tool_graph_hierarchy(&arguments),
            "codesynapse_list_graphs" => self.tool_graph_list_graphs(&arguments),
            "codesynapse_module_summary" => self.tool_graph_module_summary(&arguments),
            "codesynapse_outline" => self.tool_codesynapse_outline(&arguments),
            "codesynapse_read" => self.tool_codesynapse_read(&arguments),
            "codesynapse_read_method" => self.tool_codesynapse_read_method(&arguments),
            "codesynapse_read_with_callees" => self.tool_codesynapse_read_with_callees(&arguments),
            "codesynapse_find_callers" => self.tool_codesynapse_find_callers(&arguments),
            "codesynapse_find_usages" => self.tool_codesynapse_find_usages(&arguments),
            "codesynapse_context" => self.tool_codesynapse_context(&arguments),
            "codesynapse_resolve" => self.tool_codesynapse_resolve(&arguments),
            "codesynapse_build" => self.tool_graph_build(&arguments),
            "codesynapse_stats" => self.tool_codesynapse_stats(&arguments),
            other => return Err(format!("Unknown tool: {}", other)),
        }?;

        let text = if self.check_stale_once() {
            format!(
                "> ⚠ Graph may be stale — run `codesynapse build` or `codesynapse watch` to refresh.\n\n{text}"
            )
        } else {
            text
        };

        Ok(serde_json::json!({
            "content": [{"type": "text", "text": text}]
        }))
    }

    fn is_graph_stale(&self) -> bool {
        let global_graph = self.global_dir.join("global-graph.json");
        let Ok(gg_meta) = std::fs::metadata(&global_graph) else {
            return false;
        };
        let Ok(gg_mtime) = gg_meta.modified() else {
            return false;
        };
        let modules_dir = self.global_dir.join("modules");
        let Ok(entries) = std::fs::read_dir(&modules_dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let graph_json = entry.path().join("graph.json");
            if let Ok(meta) = std::fs::metadata(&graph_json) {
                if let Ok(mtime) = meta.modified() {
                    if mtime > gg_mtime {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn check_stale_once(&self) -> bool {
        let mut guard = self.stale_check.lock().unwrap();
        if let Some(v) = *guard {
            return v;
        }
        let stale = self.is_graph_stale();
        *guard = Some(stale);
        stale
    }

    fn load_graph(&self) -> std::result::Result<(Vec<Node>, Vec<Edge>), String> {
        let backend = self.backend.lock().unwrap();
        let nodes = backend.get_all_nodes().map_err(|e| e.to_string())?;
        let edges = backend.get_all_edges().map_err(|e| e.to_string())?;
        Ok((nodes, edges))
    }

    fn serve_graph_to_nodes_edges(g: &ServeGraph) -> (Vec<Node>, Vec<Edge>) {
        let nodes: Vec<Node> = g
            .nodes_iter()
            .map(|(id, n)| Node {
                id: id.to_string(),
                label: n.label.clone(),
                file_type: "code".to_string(),
                source_file: n.source_file.clone(),
                source_location: if n.source_location.is_empty() {
                    None
                } else {
                    Some(n.source_location.clone())
                },
                community: n.community.map(|c| c as usize),
                rationale: None,
                docstring: n.docstring.clone(),
                metadata: HashMap::new(),
            })
            .collect();
        let edges: Vec<Edge> = g
            .edges_iter()
            .map(|e: &ServeEdge| Edge {
                source: e.source.clone(),
                target: e.target.clone(),
                relation: e.relation.clone(),
                confidence: e.confidence.clone(),
                source_file: None,
                weight: 1.0,
                context: e.context.clone(),
            })
            .collect();
        (nodes, edges)
    }

    fn load_graph_with_fallback(&self) -> std::result::Result<(Vec<Node>, Vec<Edge>), String> {
        let (nodes, edges) = self.load_graph()?;
        if nodes.is_empty() {
            let g = self.load_module_serve_graph("merged")?;
            return Ok(Self::serve_graph_to_nodes_edges(&g));
        }
        Ok((nodes, edges))
    }

    fn str_arg<'a>(
        args: &'a Map<String, Value>,
        key: &str,
    ) -> std::result::Result<&'a str, String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("Missing or invalid string argument: {}", key))
    }

    fn int_arg(args: &Map<String, Value>, key: &str, default: i64) -> i64 {
        args.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
    }

    fn float_arg(args: &Map<String, Value>, key: &str, default: f64) -> f64 {
        args.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
    }

    fn tool_query_graph(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let question = Self::str_arg(args, "question")?;
        let budget = Self::int_arg(args, "budget", 5) as usize;
        let (_nodes, edges) = self.load_graph_with_fallback()?;
        // Simple keyword-based query
        let keywords: Vec<&str> = question.split_whitespace().collect();
        let matched: Vec<String> = edges
            .iter()
            .filter(|e| {
                keywords
                    .iter()
                    .any(|k| e.source.contains(k) || e.target.contains(k) || e.relation.contains(k))
            })
            .take(budget)
            .map(|e| format!("{} --[{}]--> {}", e.source, e.relation, e.target))
            .collect();
        if matched.is_empty() {
            Ok("No matching results found.".to_string())
        } else {
            Ok(matched.join("\n"))
        }
    }

    fn tool_get_node(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let node_id = Self::str_arg(args, "node_id")?;
        // Try sled first, fall back to merged graph
        let sled_result = self
            .backend
            .lock()
            .unwrap()
            .get_node(node_id)
            .ok()
            .flatten();
        if let Some(n) = sled_result {
            return serde_json::to_string_pretty(&n).map_err(|e| e.to_string());
        }
        match self.load_module_serve_graph("merged") {
            Ok(g) => match g.get_node(node_id) {
                Some(n) => Ok(format!(
                    "id: {}\nlabel: {}\nsource_file: {}\nsource_location: {}",
                    n.id, n.label, n.source_file, n.source_location
                )),
                None => Ok(format!("Node '{}' not found", node_id)),
            },
            Err(_) => Ok(format!("Node '{}' not found", node_id)),
        }
    }

    fn tool_get_neighbors(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let node_id = Self::str_arg(args, "node_id")?;
        let depth = Self::int_arg(args, "depth", 1) as usize;
        let limit = Self::int_arg(args, "limit", 50) as usize;
        let offset = Self::int_arg(args, "offset", 0) as usize;

        // Try sled first
        let sled_empty = self.backend.lock().unwrap().node_count().unwrap_or(0) == 0;
        if !sled_empty {
            let mut collected = Vec::new();
            let mut current = vec![node_id.to_string()];
            let mut visited = std::collections::HashSet::new();
            visited.insert(node_id.to_string());
            for _ in 0..depth {
                let mut next = Vec::new();
                for id in &current {
                    if let Ok(neighbors) = self.backend.lock().unwrap().neighbors(id, None) {
                        for (neighbor, edge) in &neighbors {
                            if visited.insert(neighbor.id.clone()) {
                                collected.push(format!(
                                    "{} --[{}]--> {}",
                                    edge.source, edge.relation, edge.target
                                ));
                                next.push(neighbor.id.clone());
                            }
                        }
                    }
                }
                current = next;
            }
            let total = collected.len();
            let slice: Vec<String> = collected.into_iter().skip(offset).take(limit).collect();
            return Ok(format!(
                "Found {} neighbors (showing {}):\n{}",
                total,
                slice.len(),
                slice.join("\n")
            ));
        }

        // Fallback: filter edges in merged graph
        let (_nodes, edges) = self.load_graph_with_fallback()?;
        let mut collected = Vec::new();
        let mut current = vec![node_id.to_string()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(node_id.to_string());
        for _ in 0..depth {
            let mut next = Vec::new();
            for id in &current {
                for e in edges.iter().filter(|e| e.source == *id || e.target == *id) {
                    let neighbor = if e.source == *id {
                        &e.target
                    } else {
                        &e.source
                    };
                    if visited.insert(neighbor.clone()) {
                        collected.push(format!("{} --[{}]--> {}", e.source, e.relation, e.target));
                        next.push(neighbor.clone());
                    }
                }
            }
            current = next;
        }
        let total = collected.len();
        let slice: Vec<String> = collected.into_iter().skip(offset).take(limit).collect();
        Ok(format!(
            "Found {} neighbors (showing {}):\n{}",
            total,
            slice.len(),
            slice.join("\n")
        ))
    }

    fn tool_get_community(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let community_id = Self::int_arg(args, "community_id", 0) as usize;
        let limit = Self::int_arg(args, "limit", 50) as usize;
        let offset = Self::int_arg(args, "offset", 0) as usize;

        let (nodes, _edges) = self.load_graph_with_fallback()?;
        let community_nodes: Vec<&Node> = nodes
            .iter()
            .filter(|n| n.community == Some(community_id))
            .skip(offset)
            .take(limit)
            .collect();

        if community_nodes.is_empty() {
            return Ok(format!("No nodes found in community {}", community_id));
        }

        let lines: Vec<String> = community_nodes
            .iter()
            .map(|n| format!("{} ({})", n.label, n.id))
            .collect();
        Ok(format!(
            "Community {} ({} nodes):\n{}",
            community_id,
            community_nodes.len(),
            lines.join("\n")
        ))
    }

    fn tool_god_nodes(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let top_n = Self::int_arg(args, "top_n", 10) as usize;
        let (nodes, edges) = self.load_graph_with_fallback()?;
        let analyzer = Analyzer;
        let gods = analyzer.god_nodes(&nodes, &edges, top_n);
        let lines: Vec<String> = gods
            .iter()
            .map(|n| format!("{} ({})", n.label, n.id))
            .collect();
        Ok(format!("Top {} god nodes:\n{}", top_n, lines.join("\n")))
    }

    fn tool_graph_stats(&self, _args: &Map<String, Value>) -> std::result::Result<String, String> {
        let (nodes, edges) = self.load_graph_with_fallback()?;
        Ok(format!("Nodes: {}\nEdges: {}", nodes.len(), edges.len()))
    }

    fn tool_shortest_path(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let source = Self::str_arg(args, "source")?;
        let target = Self::str_arg(args, "target")?;
        let path = self
            .backend
            .lock()
            .unwrap()
            .shortest_path(source, target)
            .map_err(|e| e.to_string())?;
        match path {
            Some(nodes) => {
                let labels: Vec<String> = nodes.iter().map(|n| n.label.clone()).collect();
                Ok(format!(
                    "Shortest path ({} hops): {}",
                    labels.len() - 1,
                    labels.join(" -> ")
                ))
            }
            None => Ok(format!(
                "No path found between '{}' and '{}'",
                source, target
            )),
        }
    }

    fn tool_find_all_paths(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let source = Self::str_arg(args, "source")?.to_string();
        let target = Self::str_arg(args, "target")?.to_string();
        let max_length = Self::int_arg(args, "max_length", 5) as usize;

        let (_nodes, edges) = self.load_graph_with_fallback()?;
        let adj: HashMap<&str, Vec<&str>> = {
            let mut m: HashMap<&str, Vec<&str>> = HashMap::new();
            for e in &edges {
                m.entry(e.source.as_str())
                    .or_default()
                    .push(e.target.as_str());
            }
            m
        };

        let mut all_paths: Vec<Vec<String>> = Vec::new();
        // Stack entries: (current_path, visited_set)
        let mut stack: Vec<(Vec<&str>, std::collections::HashSet<&str>)> = Vec::new();
        let mut init_visited = std::collections::HashSet::new();
        init_visited.insert(source.as_str());
        stack.push((vec![source.as_str()], init_visited));

        while let Some((current, visited)) = stack.pop() {
            if current.len() > max_length {
                continue;
            }
            let node = *current.last().unwrap();
            if node == target.as_str() {
                all_paths.push(current.iter().map(|s| s.to_string()).collect());
                continue;
            }
            if let Some(neighbors) = adj.get(node) {
                for &next in neighbors {
                    if !visited.contains(next) {
                        let mut new_path = current.clone();
                        new_path.push(next);
                        let mut new_visited = visited.clone();
                        new_visited.insert(next);
                        stack.push((new_path, new_visited));
                    }
                }
            }
        }

        if all_paths.is_empty() {
            return Ok(format!(
                "No paths found between '{}' and '{}'",
                source, target
            ));
        }

        let lines: Vec<String> = all_paths
            .iter()
            .enumerate()
            .map(|(i, p)| format!("  {}. {}", i + 1, p.join(" -> ")))
            .collect();
        Ok(format!(
            "Found {} paths:\n{}",
            all_paths.len(),
            lines.join("\n")
        ))
    }

    fn tool_weighted_path(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let source = Self::str_arg(args, "source")?;
        let target = Self::str_arg(args, "target")?;
        let _min_confidence = Self::float_arg(args, "min_confidence", 0.0);
        let path = self
            .backend
            .lock()
            .unwrap()
            .dijkstra_shortest_path(source, target)
            .map_err(|e| e.to_string())?;
        match path {
            Some(nodes) => {
                let labels: Vec<String> = nodes.iter().map(|n| n.label.clone()).collect();
                Ok(format!(
                    "Weighted path ({} hops): {}",
                    labels.len() - 1,
                    labels.join(" -> ")
                ))
            }
            None => Ok(format!(
                "No path found between '{}' and '{}'",
                source, target
            )),
        }
    }

    fn tool_community_bridges(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let top_n = Self::int_arg(args, "top_n", 10) as usize;
        let (nodes, edges) = self.load_graph_with_fallback()?;
        let analyzer = Analyzer;
        let bridges = analyzer.bridge_edges(&nodes, &edges);
        let top: Vec<&Edge> = bridges.iter().take(top_n).collect();
        if top.is_empty() {
            return Ok("No bridge edges found.".to_string());
        }
        let lines: Vec<String> = top
            .iter()
            .map(|e| format!("{} --[{}]--> {}", e.source, e.relation, e.target))
            .collect();
        Ok(format!(
            "Top {} bridge edges:\n{}",
            top.len(),
            lines.join("\n")
        ))
    }

    fn tool_graph_diff(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let other_path = Self::str_arg(args, "other_graph")?;
        let other_store = SledGraphStore::open(Path::new(other_path)).map_err(|e| e.to_string())?;
        let other_backend = StoreBackend::Sled(other_store);
        let other_edges = other_backend.get_all_edges().map_err(|e| e.to_string())?;
        let (_nodes, edges) = self.load_graph_with_fallback()?;
        let analyzer = Analyzer;
        let (added, removed) = analyzer.graph_diff(&other_edges, &edges);
        Ok(format!(
            "Added edges: {}\nRemoved edges: {}",
            added.len(),
            removed.len()
        ))
    }

    fn tool_pagerank(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let top_n = Self::int_arg(args, "top_n", 10) as usize;
        let damping = Self::float_arg(args, "damping", 0.85);
        let max_iter = Self::int_arg(args, "max_iter", 100) as usize;
        let (_nodes, edges) = self.load_graph_with_fallback()?;
        let analyzer = Analyzer;
        let mut ranks = analyzer.pagerank(&edges, damping, max_iter);
        let mut sorted: Vec<(String, f64)> = ranks.drain().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top: Vec<String> = sorted
            .iter()
            .take(top_n)
            .map(|(id, score)| format!("{}: {:.6}", id, score))
            .collect();
        Ok(format!("Top {} PageRank:\n{}", top_n, top.join("\n")))
    }

    fn tool_detect_cycles(
        &self,
        _args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let (_nodes, edges) = self.load_graph_with_fallback()?;
        let analyzer = Analyzer;
        let sccs = analyzer.tarjan_scc(&edges);
        if sccs.is_empty() {
            return Ok("No cycles detected.".to_string());
        }
        let lines: Vec<String> = sccs
            .iter()
            .filter(|scc| scc.len() > 1)
            .enumerate()
            .map(|(i, scc)| format!("  Cycle {}: {}", i + 1, scc.join(" -> ")))
            .collect();
        if lines.is_empty() {
            return Ok("No cycles detected (all SCCs are singletons).".to_string());
        }
        Ok(format!(
            "Detected {} cycles:\n{}",
            lines.len(),
            lines.join("\n")
        ))
    }

    fn tool_smart_summary(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let level = args
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("architecture");
        let budget = Self::int_arg(args, "budget", 100) as usize;
        let (nodes, edges) = self.load_graph_with_fallback()?;

        match level {
            "detailed" => {
                let mut lines = vec![format!(
                    "Detailed summary ({} nodes, {} edges, max {} items):",
                    nodes.len(),
                    edges.len(),
                    budget
                )];
                for node in nodes.iter().take(budget / 2) {
                    lines.push(format!("  Node: {} ({})", node.label, node.id));
                }
                for edge in edges.iter().take(budget / 2) {
                    lines.push(format!(
                        "  Edge: {} --[{}]--> {}",
                        edge.source, edge.relation, edge.target
                    ));
                }
                Ok(lines.join("\n"))
            }
            "community" => {
                let mut community_map: HashMap<Option<usize>, Vec<&Node>> = HashMap::new();
                for node in &nodes {
                    community_map.entry(node.community).or_default().push(node);
                }
                let mut lines = vec![format!(
                    "Community summary ({} communities):",
                    community_map.len()
                )];
                for (id, members) in community_map.iter().take(budget) {
                    lines.push(format!("  Community {:?}: {} members", id, members.len()));
                }
                Ok(lines.join("\n"))
            }
            _ => {
                // architecture level
                let node_count = nodes.len();
                let edge_count = edges.len();
                let analyzer = Analyzer;
                let gods = analyzer.god_nodes(&nodes, &edges, 5);
                let sccs = analyzer.tarjan_scc(&edges);
                let has_cycles = sccs.iter().any(|scc| scc.len() > 1);
                let god_lines: Vec<String> = gods.iter().map(|n| n.label.clone()).collect();
                Ok(format!(
                    "Architecture Summary:\n\
                     - Nodes: {}\n\
                     - Edges: {}\n\
                     - Cycles: {}\n\
                     - Top nodes: {}\n\
                     - Average degree: {:.2}",
                    node_count,
                    edge_count,
                    if has_cycles { "yes" } else { "no" },
                    god_lines.join(", "),
                    if node_count > 0 {
                        edge_count as f64 / node_count as f64
                    } else {
                        0.0
                    }
                ))
            }
        }
    }

    fn tool_find_similar(&self, args: &Map<String, Value>) -> std::result::Result<String, String> {
        let node_id = Self::str_arg(args, "node_id")?;
        let top_n = Self::int_arg(args, "top_n", 10) as usize;

        let (_nodes, edges) = self.load_graph_with_fallback()?;
        let analyzer = Analyzer;
        let similar = analyzer.find_similar(&edges, node_id, top_n);

        if similar.is_empty() {
            Ok(format!("No similar nodes found for '{}'", node_id))
        } else {
            let lines: Vec<String> = similar
                .iter()
                .map(|(id, score)| format!("  {} (similarity: {:.4})", id, score))
                .collect();
            Ok(format!(
                "Top {} nodes similar to '{}':\n{}",
                similar.len(),
                node_id,
                lines.join("\n")
            ))
        }
    }

    fn load_module_serve_graph(
        &self,
        module: &str,
    ) -> std::result::Result<Arc<ServeGraph>, String> {
        let path = if module == "merged" || module.is_empty() {
            let p = self.global_dir.join("global-graph.json");
            if !p.exists() {
                return Err("No merged graph found. Run `codesynapse global add <path>` to register graphs.".into());
            }
            p
        } else {
            let repos = global_list(&self.global_dir);
            let entry = repos.get(module).ok_or_else(|| {
                format!(
                    "Module '{}' not found. Use graph_list_graphs to see available modules.",
                    module
                )
            })?;
            PathBuf::from(&entry.source_path)
        };

        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        {
            let cache = self.graph_cache.lock().unwrap();
            if let Some((cached_mtime, arc)) = cache.get(module) {
                if *cached_mtime == mtime {
                    return Ok(Arc::clone(arc));
                }
            }
        }

        let g = load_graph(&path).map_err(|e| e.to_string())?;
        let arc = Arc::new(g);
        self.graph_cache
            .lock()
            .unwrap()
            .insert(module.to_string(), (mtime, Arc::clone(&arc)));
        Ok(arc)
    }

    fn tool_graph_list_graphs(
        &self,
        _args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let repos = global_list(&self.global_dir);
        if repos.is_empty() {
            return Ok("No graphs registered. Use `codesynapse global add <path> --as <tag>` to register one.".into());
        }
        let mut lines = vec![
            format!(
                "{:<30} {:>7}  {:>7}  {}",
                "Module", "Nodes", "Edges", "Source"
            ),
            format!("{}", "─".repeat(80)),
        ];
        let mut sorted: Vec<_> = repos.iter().collect();
        sorted.sort_by_key(|(k, _)| k.as_str());
        for (tag, entry) in sorted {
            lines.push(format!(
                "{:<30} {:>7}  {:>7}  {}",
                tag, entry.node_count, entry.edge_count, entry.source_path
            ));
        }
        Ok(lines.join("\n"))
    }

    fn tool_graph_module_summary(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let module = Self::str_arg(args, "module")?;
        let repos = global_list(&self.global_dir);
        let entry = repos
            .get(module)
            .ok_or_else(|| format!("Module '{}' not found.", module))?;

        let text = std::fs::read_to_string(&entry.source_path)
            .map_err(|e| format!("Could not read graph: {}", e))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("Bad graph JSON: {}", e))?;

        let nodes: Vec<Node> = v
            .get("nodes")
            .and_then(|n| n.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| serde_json::from_value(i.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();
        let edges: Vec<Edge> = v
            .get("edges")
            .or_else(|| v.get("links"))
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| serde_json::from_value(i.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        let analyzer = Analyzer;
        let gods = analyzer.god_nodes(&nodes, &edges, 5);
        let god_labels: Vec<String> = gods.iter().map(|n| n.label.clone()).collect();

        let mut lang_counts: HashMap<&str, usize> = HashMap::new();
        for n in &nodes {
            *lang_counts.entry(n.file_type.as_str()).or_default() += 1;
        }
        let mut lang_sorted: Vec<_> = lang_counts.iter().collect();
        lang_sorted.sort_by(|a, b| b.1.cmp(a.1));
        let lang_str: Vec<String> = lang_sorted
            .iter()
            .take(5)
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();

        Ok(format!(
            "Module:     {}\nNodes:      {}\nEdges:      {}\nTop nodes:  {}\nLanguages:  {}\nSource:     {}",
            module,
            entry.node_count,
            entry.edge_count,
            if god_labels.is_empty() { "(none)".to_string() } else { god_labels.join(", ") },
            if lang_str.is_empty() { "(none)".to_string() } else { lang_str.join(", ") },
            entry.source_path,
        ))
    }

    fn tool_graph_blast_radius(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let depth = Self::int_arg(args, "depth", 3) as usize;

        let g = self.load_module_serve_graph(graph_name)?;

        let seed_id = g
            .nodes_iter()
            .find(|(id, n)| n.label.eq_ignore_ascii_case(class_name) || id.contains(class_name))
            .map(|(id, _)| id.to_string())
            .ok_or_else(|| format!("Node '{}' not found in graph '{}'.", class_name, graph_name))?;

        let mut by_depth: Vec<Vec<String>> = vec![vec![seed_id.clone()]];
        let mut visited = std::collections::HashSet::new();
        visited.insert(seed_id.clone());

        for d in 1..=depth {
            let prev = by_depth[d - 1].clone();
            let mut next = Vec::new();
            for node_id in &prev {
                for neighbor_id in g.neighbors(node_id) {
                    if visited.insert(neighbor_id.clone()) {
                        next.push(neighbor_id.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            by_depth.push(next);
        }

        let mut lines = vec![format!(
            "Blast radius of '{}' in '{}' (max depth {})",
            class_name, graph_name, depth
        )];
        for (d, nodes) in by_depth.iter().enumerate() {
            let labels: Vec<String> = nodes
                .iter()
                .map(|id| {
                    g.get_node(id)
                        .map(|n| n.label.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect();
            if d == 0 {
                lines.push(format!("  Origin:  {}", labels.join(", ")));
            } else {
                lines.push(format!("  Depth {}: {}", d, labels.join(", ")));
            }
        }
        lines.push(format!("Total affected: {}", visited.len() - 1));
        Ok(lines.join("\n"))
    }

    fn risk_score_node(g: &ServeGraph, node_id: &str) -> (f32, Vec<String>) {
        const KEYWORDS: &[&str] = &[
            "auth",
            "pay",
            "password",
            "token",
            "secret",
            "credit",
            "billing",
            "admin",
            "security",
            "encrypt",
            "hash",
            "key",
            "credential",
        ];
        let node = g.get_node(node_id);
        let label_lower = node.map(|n| n.label.to_lowercase()).unwrap_or_default();
        let file_lower = node
            .map(|n| n.source_file.to_lowercase())
            .unwrap_or_default();
        let combined = format!("{} {}", label_lower, file_lower);

        let mut score = 0.0f32;
        let mut reasons = Vec::new();

        let kw_hits: Vec<&str> = KEYWORDS
            .iter()
            .copied()
            .filter(|k| combined.contains(k))
            .collect();
        if !kw_hits.is_empty() {
            let kw_score = (kw_hits.len() as f32 * 0.15).min(0.4);
            score += kw_score;
            reasons.push(format!("keywords: {}", kw_hits.join(",")));
        }

        let in_degree = g.edges_iter().filter(|e| e.target == node_id).count();
        if in_degree >= 10 {
            score += 0.3;
            reasons.push(format!("in_degree={}", in_degree));
        } else if in_degree >= 5 {
            score += 0.15;
            reasons.push(format!("in_degree={}", in_degree));
        }

        let has_test_neighbor = g.edges_iter().any(|e| {
            if e.source == node_id || e.target == node_id {
                let neighbor_id = if e.source == node_id {
                    &e.target
                } else {
                    &e.source
                };
                g.get_node(neighbor_id)
                    .map(|n| {
                        let f = &n.source_file;
                        f.contains("test")
                            || f.contains("spec")
                            || f.contains("Test")
                            || f.contains("_test")
                    })
                    .unwrap_or(false)
            } else {
                false
            }
        });
        if !has_test_neighbor {
            score += 0.2;
            reasons.push("no_test_coverage".into());
        }

        (score.min(1.0), reasons)
    }

    fn tool_graph_blast_radius_scored(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let depth = Self::int_arg(args, "depth", 3) as usize;

        let g = self.load_module_serve_graph(graph_name)?;

        let seed_id = g
            .nodes_iter()
            .find(|(id, n)| n.label.eq_ignore_ascii_case(class_name) || id.contains(class_name))
            .map(|(id, _)| id.to_string())
            .ok_or_else(|| format!("Node '{}' not found in graph '{}'.", class_name, graph_name))?;

        let mut by_depth: Vec<Vec<String>> = vec![vec![seed_id.clone()]];
        let mut visited = std::collections::HashSet::new();
        visited.insert(seed_id.clone());

        for d in 1..=depth {
            let prev = by_depth[d - 1].clone();
            let mut next = Vec::new();
            for node_id in &prev {
                for neighbor_id in g.neighbors(node_id) {
                    if visited.insert(neighbor_id.clone()) {
                        next.push(neighbor_id.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            by_depth.push(next);
        }

        let affected: Vec<String> = visited
            .iter()
            .filter(|id| *id != &seed_id)
            .cloned()
            .collect();

        let mut scored: Vec<(String, f32, Vec<String>)> = affected
            .iter()
            .map(|id| {
                let label = g
                    .get_node(id)
                    .map(|n| n.label.clone())
                    .unwrap_or_else(|| id.clone());
                let (score, reasons) = Self::risk_score_node(&g, id);
                (label, score, reasons)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut lines = vec![format!(
            "Blast radius of '{}' in '{}' (max depth {}) — risk scored",
            class_name, graph_name, depth
        )];
        let tier = |s: f32| {
            if s >= 0.6 {
                "HIGH"
            } else if s >= 0.3 {
                "MEDIUM"
            } else {
                "LOW"
            }
        };
        for (label, score, reasons) in &scored {
            let reason_str = if reasons.is_empty() {
                String::new()
            } else {
                format!(" [{}]", reasons.join(", "))
            };
            lines.push(format!(
                "  [{:.2} {}] {}{}",
                score,
                tier(*score),
                label,
                reason_str
            ));
        }
        lines.push(format!("Total affected: {}", affected.len()));
        Ok(lines.join("\n"))
    }

    fn tool_graph_blast_radius_multi(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let depth = Self::int_arg(args, "depth", 3) as usize;

        let names_val = args
            .get("class_names")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "Missing required argument: class_names".to_string())?;
        let class_names: Vec<String> = names_val
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();

        if class_names.is_empty() {
            return Err("class_names must be a non-empty array".to_string());
        }

        let g = self.load_module_serve_graph(graph_name)?;

        let mut seed_ids: Vec<String> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();
        for name in &class_names {
            match g
                .nodes_iter()
                .find(|(id, n)| n.label.eq_ignore_ascii_case(name) || id.contains(name.as_str()))
            {
                Some((id, _)) => seed_ids.push(id.to_string()),
                None => not_found.push(name.clone()),
            }
        }

        let mut by_depth: Vec<Vec<String>> = vec![seed_ids.clone()];
        let mut visited: std::collections::HashSet<String> = seed_ids.iter().cloned().collect();

        for d in 1..=depth {
            let prev = by_depth[d - 1].clone();
            let mut next = Vec::new();
            for node_id in &prev {
                for neighbor_id in g.neighbors(node_id) {
                    if visited.insert(neighbor_id.clone()) {
                        next.push(neighbor_id.clone());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            by_depth.push(next);
        }

        let seed_set: std::collections::HashSet<&String> = seed_ids.iter().collect();
        let total_affected = visited.iter().filter(|id| !seed_set.contains(*id)).count();

        let mut lines = vec![format!(
            "Multi blast radius in '{}' (seeds: {}, max depth {})",
            graph_name,
            class_names.join(", "),
            depth
        )];
        if !not_found.is_empty() {
            lines.push(format!("Not found: {}", not_found.join(", ")));
        }
        for (d, nodes) in by_depth.iter().enumerate() {
            let labels: Vec<String> = nodes
                .iter()
                .map(|id| {
                    g.get_node(id)
                        .map(|n| n.label.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect();
            if d == 0 {
                lines.push(format!("  Seeds:   {}", labels.join(", ")));
            } else {
                lines.push(format!("  Depth {}: {}", d, labels.join(", ")));
            }
        }
        lines.push(format!("Total affected: {}", total_affected));
        Ok(lines.join("\n"))
    }

    fn tool_graph_query_semantic(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let query = Self::str_arg(args, "query")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let depth = Self::int_arg(args, "depth", 2) as usize;
        let min_confidence = args
            .get("min_confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.7) as f32;

        let g = self.load_module_serve_graph(graph_name)?;

        let query_lower = query.to_lowercase();
        let seed_ids: Vec<String> = {
            let mut seeds: Vec<String> = g
                .nodes_iter()
                .filter(|(_, n)| {
                    let l = n.label.to_lowercase();
                    query_lower.split_whitespace().any(|w| l.contains(w))
                })
                .take(5)
                .map(|(id, _)| id.to_string())
                .collect();
            if seeds.is_empty() {
                seeds = find_nodes_by_label(&g, query)
                    .into_iter()
                    .take(5)
                    .map(|(id, _)| id.to_string())
                    .collect();
            }
            seeds
        };

        if seed_ids.is_empty() {
            return Ok(format!("No seed nodes found for query '{}'.", query));
        }

        let has_any_semantic = g
            .edges_iter()
            .any(|e| e.relation == "semantically_similar_to");
        if !has_any_semantic {
            return Ok(format!(
                "No semantic neighbors found for '{}'. Graph may not have been built with --llm.",
                query
            ));
        }

        let mut visited: std::collections::HashSet<String> = seed_ids.iter().cloned().collect();
        let mut frontier: Vec<String> = seed_ids.clone();
        let mut results: Vec<(String, usize)> = Vec::new();

        for d in 1..=depth {
            let mut next_frontier = Vec::new();
            for node_id in &frontier {
                for edge in g.edges_iter() {
                    if edge.relation != "semantically_similar_to" {
                        continue;
                    }
                    let conf: f32 = edge.confidence.parse().unwrap_or(1.0);
                    if conf < min_confidence {
                        continue;
                    }
                    let neighbor = if edge.source == *node_id {
                        Some(edge.target.clone())
                    } else if edge.target == *node_id {
                        Some(edge.source.clone())
                    } else {
                        None
                    };
                    if let Some(nb) = neighbor {
                        if visited.insert(nb.clone()) {
                            next_frontier.push(nb.clone());
                            results.push((nb, d));
                        }
                    }
                }
            }
            frontier = next_frontier;
            if frontier.is_empty() {
                break;
            }
        }

        if results.is_empty() {
            return Ok(format!(
                "No semantic neighbors found for '{}'. Graph may not have been built with --llm.",
                query
            ));
        }

        let seed_labels: Vec<String> = seed_ids
            .iter()
            .map(|id| {
                g.get_node(id)
                    .map(|n| n.label.clone())
                    .unwrap_or_else(|| id.clone())
            })
            .collect();

        let mut lines = vec![format!(
            "Semantic neighbors of '{}' (seeds: {}, depth {}, min_confidence {:.2})",
            query,
            seed_labels.join(", "),
            depth,
            min_confidence
        )];
        for (id, d) in &results {
            let label = g
                .get_node(id)
                .map(|n| n.label.clone())
                .unwrap_or_else(|| id.clone());
            lines.push(format!("  [depth {}] {}", d, label));
        }
        Ok(lines.join("\n"))
    }

    fn tool_graph_hierarchy(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");

        let g = self.load_module_serve_graph(graph_name)?;

        let seed_id = g
            .nodes_iter()
            .find(|(id, n)| n.label.eq_ignore_ascii_case(class_name) || id.contains(class_name))
            .map(|(id, _)| id.to_string())
            .ok_or_else(|| format!("Node '{}' not found in graph '{}'.", class_name, graph_name))?;

        let seed_label = g
            .get_node(&seed_id)
            .map(|n| n.label.clone())
            .unwrap_or_else(|| seed_id.clone());

        let hierarchy_rels = ["extends", "implements", "inherits"];

        let mut supertypes: Vec<String> = Vec::new();
        let mut subtypes: Vec<String> = Vec::new();

        for edge in g.edges_iter() {
            let rel = edge.relation.to_lowercase();
            if !hierarchy_rels.contains(&rel.as_str()) {
                continue;
            }
            if edge.source == seed_id {
                let label = g
                    .get_node(&edge.target)
                    .map(|n| n.label.clone())
                    .unwrap_or_else(|| edge.target.clone());
                supertypes.push(label);
            }
            if edge.target == seed_id {
                let label = g
                    .get_node(&edge.source)
                    .map(|n| n.label.clone())
                    .unwrap_or_else(|| edge.source.clone());
                subtypes.push(label);
            }
        }

        let mut lines = vec![format!("Hierarchy for '{}':", seed_label)];
        if supertypes.is_empty() {
            lines.push("  Supertypes: (none)".into());
        } else {
            lines.push(format!("  Supertypes: {}", supertypes.join(", ")));
        }
        lines.push(format!("  ↳ {}", seed_label));
        if subtypes.is_empty() {
            lines.push("  Subtypes: (none)".into());
        } else {
            for sub in &subtypes {
                lines.push(format!("    ↳ {}", sub));
            }
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn get_source_cached(&self, path: &PathBuf) -> Option<(String, Vec<OutlineItem>)> {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;

        {
            let cache = self.source_cache.lock().unwrap();
            if let Some((cached_mtime, content, items)) = cache.get(path) {
                if *cached_mtime == mtime {
                    return Some((content.clone(), items.clone()));
                }
            }
        }

        let content = std::fs::read_to_string(path).ok()?;
        let items = outline_items(path, &content);

        let mut cache = self.source_cache.lock().unwrap();
        if cache.len() >= 1000 {
            cache.clear();
        }
        cache.insert(path.clone(), (mtime, content.clone(), items.clone()));
        Some((content, items))
    }

    fn tool_graph_query_vector(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let query = Self::str_arg(args, "query")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let top_k = Self::int_arg(args, "top_k", 8) as usize;

        let g = self.load_module_serve_graph(graph_name)?;
        let dense = self
            .embedder
            .as_ref()
            .filter(|_| !self.node_embeddings.is_empty())
            .map(|e| (e, &self.node_embeddings));
        let result = query_graph_text_hybrid(&g, query, "bfs", 2, top_k * 200, None, dense);
        Ok(result)
    }

    // ---------------------------------------------------------------------------
    // Phase 3 & 4 — File tools + utility tools
    // ---------------------------------------------------------------------------

    fn log_tool_call(&self, tool: &str, result_chars: usize, saved_chars: usize) {
        use std::io::Write as IoWrite;
        let path = self.global_dir.join("tool_stats.jsonl");
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let entry = serde_json::json!({
            "ts": ts,
            "tool": tool,
            "result_chars": result_chars,
            "saved_chars": saved_chars,
        });
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{}", entry);
        }
        self.telemetry.record_usage(tool, saved_chars, true);
    }

    fn resolve_class_source(
        &self,
        g: &ServeGraph,
        class_name: &str,
        method_hint: Option<&str>,
    ) -> std::result::Result<PathBuf, String> {
        let candidates = find_nodes_by_label(g, class_name);
        if candidates.is_empty() {
            return Err(format!("No node matching '{}' found in graph.", class_name));
        }
        let mut paths: Vec<PathBuf> = Vec::new();
        for (id, node) in candidates.iter().take(10) {
            if !node.source_file.is_empty() {
                let p = PathBuf::from(&node.source_file);
                if p.exists() {
                    paths.push(p);
                    continue;
                }
            }
            for neighbor_id in g.neighbors(id) {
                if let Some(neighbor) = g.get_node(neighbor_id) {
                    if !neighbor.source_file.is_empty() {
                        let p = PathBuf::from(&neighbor.source_file);
                        if p.exists() {
                            paths.push(p);
                            break;
                        }
                    }
                }
            }
        }
        if paths.is_empty() {
            return Err(format!(
                "Found '{}' in graph but source file not found on disk.",
                class_name
            ));
        }
        if let Some(mhint) = method_hint {
            if paths.len() > 1 {
                let mhint_lower = mhint.to_lowercase();
                for p in &paths {
                    if let Ok(content) = std::fs::read_to_string(p) {
                        let items = outline_items(p, &content);
                        if items.iter().any(|i| {
                            i.kind == "method" && i.name.to_lowercase().contains(&mhint_lower)
                        }) {
                            return Ok(p.clone());
                        }
                    }
                }
            }
        }
        Ok(paths[0].clone())
    }

    fn tool_codesynapse_outline(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let g = self.load_module_serve_graph(graph_name)?;
        let path = self.resolve_class_source(&g, class_name, None)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        let items = outline_items(&path, &content);
        if items.is_empty() {
            return Ok(format!(
                "Could not extract outline from {} (unsupported format or empty).",
                path.display()
            ));
        }
        let total_lines = content.lines().count();
        let mut lines = vec![
            format!("File: {}", path.display()),
            format!("Size: {} lines", total_lines),
            String::new(),
        ];
        for item in &items {
            let prefix = match item.kind.as_str() {
                "class" => "CLASS ",
                "interface" => "IFACE ",
                "enum" => "ENUM  ",
                "record" => "RECORD",
                "method" => "  def ",
                "field" => "  var ",
                _ => "  ??? ",
            };
            lines.push(format!("  L{:<6} {} {}", item.line, prefix, item.name));
        }
        let result = lines.join("\n");
        self.log_tool_call(
            "codesynapse_outline",
            result.len(),
            content.len().saturating_sub(result.len()),
        );
        Ok(result)
    }

    fn tool_codesynapse_read(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let from_line = Self::int_arg(args, "from_line", 1) as usize;
        let mut to_line = Self::int_arg(args, "to_line", 0) as usize;

        let g = self.load_module_serve_graph(graph_name)?;
        let path = self.resolve_class_source(&g, class_name, None)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        let all_lines: Vec<&str> = content.lines().collect();
        let total = all_lines.len();
        if to_line == 0 {
            to_line = total;
        }
        if from_line > to_line {
            return Err(format!(
                "Invalid range: from_line ({}) > to_line ({})",
                from_line, to_line
            ));
        }
        let s = from_line.saturating_sub(1);
        let e = to_line.min(total);
        let numbered: Vec<String> = all_lines[s..e]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:<6} {}", from_line + i, line))
            .collect();
        let result = format!(
            "File: {}  ({} lines total)\n\n── lines {}-{} ──\n{}",
            path.display(),
            total,
            from_line,
            to_line,
            numbered.join("\n")
        );
        self.log_tool_call(
            "codesynapse_read",
            result.len(),
            content.len().saturating_sub(result.len()),
        );
        Ok(result)
    }

    fn tool_codesynapse_read_method(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let method_name = Self::str_arg(args, "method_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");

        let g = self.load_module_serve_graph(graph_name)?;
        let path = self.resolve_class_source(&g, class_name, Some(method_name))?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        let source_lines: Vec<&str> = content.lines().collect();
        let items = outline_items(&path, &content);

        match extract_method_range(&source_lines, &items, method_name) {
            None => {
                let methods: Vec<&str> = items
                    .iter()
                    .filter(|i| i.kind == "method")
                    .map(|i| i.name.as_str())
                    .take(20)
                    .collect();
                Err(format!(
                    "No method matching '{}' in {}.\nAvailable methods: {}",
                    method_name,
                    class_name,
                    methods.join(", ")
                ))
            }
            Some((start_1, end_1)) => {
                let numbered: Vec<String> = source_lines[start_1 - 1..end_1]
                    .iter()
                    .enumerate()
                    .map(|(i, line)| format!("{:<6} {}", start_1 + i, line))
                    .collect();
                let result = format!(
                    "File: {}  (lines {}-{} of {})\n\n{}",
                    path.display(),
                    start_1,
                    end_1,
                    source_lines.len(),
                    numbered.join("\n")
                );
                self.log_tool_call(
                    "codesynapse_read_method",
                    result.len(),
                    content.len().saturating_sub(result.len()),
                );
                Ok(result)
            }
        }
    }

    fn tool_codesynapse_read_with_callees(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let method_name = Self::str_arg(args, "method_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");

        let g = self.load_module_serve_graph(graph_name)?;
        let path = self.resolve_class_source(&g, class_name, Some(method_name))?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        let source_lines: Vec<&str> = content.lines().collect();
        let items = outline_items(&path, &content);

        let mut all_methods: HashMap<String, (usize, usize)> = HashMap::new();
        for item in &items {
            if item.kind == "method" {
                if let Some(range) =
                    extract_method_range(&source_lines, std::slice::from_ref(item), &item.name)
                {
                    all_methods.insert(item.name.clone(), range);
                }
            }
        }

        let (start_1, end_1) = match extract_method_range(&source_lines, &items, method_name) {
            None => {
                let methods: Vec<&str> = all_methods.keys().map(|s| s.as_str()).take(20).collect();
                return Err(format!(
                    "No method matching '{}' in {}.\nAvailable methods: {}",
                    method_name,
                    class_name,
                    methods.join(", ")
                ));
            }
            Some(r) => r,
        };

        let render_method = |name: &str, s1: usize, e1: usize, indent: &str| -> String {
            let lines: Vec<String> = source_lines[s1 - 1..e1]
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{}{:<6} {}", indent, s1 + i, line))
                .collect();
            format!(
                "{}--- {} (L{}-{}) ---\n{}",
                indent,
                name,
                s1,
                e1,
                lines.join("\n")
            )
        };

        let mut sections = vec![render_method(method_name, start_1, end_1, "")];

        let body = source_lines[start_1 - 1..end_1].join("\n");
        let mut callee_keys: Vec<String> = all_methods
            .keys()
            .filter(|&name| name != method_name && body.contains(&format!("{}(", name)))
            .cloned()
            .collect();
        callee_keys.sort();
        let mut seen = std::collections::HashSet::new();
        for callee_name in callee_keys {
            if seen.insert(callee_name.clone()) {
                if let Some(&(cs1, ce1)) = all_methods.get(&callee_name) {
                    sections.push(render_method(&callee_name, cs1, ce1, "  "));
                }
            }
        }

        Ok(format!(
            "File: {}\n\n{}",
            path.display(),
            sections.join("\n\n")
        ))
    }

    fn tool_codesynapse_find_callers(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let method_name = args
            .get("method_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");

        let g = self.load_module_serve_graph(graph_name)?;
        let candidates = find_nodes_by_label(&g, class_name);
        if candidates.is_empty() {
            return Ok(format!(
                "No node matching '{}' found in graph '{}'.",
                class_name, graph_name
            ));
        }
        let target_ids: std::collections::HashSet<String> = candidates
            .iter()
            .take(5)
            .map(|(id, _)| id.to_string())
            .collect();

        let call_rels = ["calls", "uses", "invokes"];
        let mut callers: Vec<(String, String)> = Vec::new();
        for edge in g.edges_iter() {
            if !target_ids.contains(&edge.target) {
                continue;
            }
            if !call_rels.contains(&edge.relation.as_str()) {
                continue;
            }
            if let Some(caller_node) = g.get_node(&edge.source) {
                callers.push((caller_node.label.clone(), caller_node.source_file.clone()));
            }
        }

        let subject = if method_name.is_empty() {
            class_name.to_string()
        } else {
            format!("{}.{}", class_name, method_name)
        };

        if !callers.is_empty() {
            let mut lines = vec![format!(
                "Callers of {} ({} found via graph call edges):\n",
                subject,
                callers.len()
            )];
            for (label, src) in &callers {
                lines.push(format!("  {}", label));
                if !src.is_empty() {
                    lines.push(format!("    [{}]", src));
                }
            }
            return Ok(lines.join("\n"));
        }

        let roots = source_roots_from_graph(&g);
        if roots.is_empty() {
            return Ok(format!(
                "No graph call edges found for {}. No source roots detected.",
                subject
            ));
        }
        let search_term = if method_name.is_empty() {
            class_name
        } else {
            method_name
        };
        let pattern = Regex::new(&format!(r"\b{}\b", regex::escape(search_term)))
            .map_err(|e| e.to_string())?;
        let exts = ["java", "js", "ts", "tsx", "py", "kt", "rs", "go", "rb"];
        let hits = search_source_files(&roots, &exts, &pattern, 50);

        if hits.is_empty() {
            return Ok(format!(
                "No callers found for {} (graph edges + source text search).",
                subject
            ));
        }
        let mut lines = vec![format!(
            "Callers of {} ({} source matches — no graph call edges):\n",
            subject,
            hits.len()
        )];
        for (rel, lineno, content) in &hits {
            lines.push(format!("  {}:{}", rel, lineno));
            lines.push(format!("    {}", content));
        }
        Ok(lines.join("\n"))
    }

    fn tool_codesynapse_find_usages(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let class_name = Self::str_arg(args, "class_name")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");

        let g = self.load_module_serve_graph(graph_name)?;
        let roots = source_roots_from_graph(&g);
        if roots.is_empty() {
            return Ok(
                "No source roots detected in graph. Ensure graph was built with absolute paths."
                    .into(),
            );
        }
        let pattern = Regex::new(&format!(r"\b{}\b", regex::escape(class_name)))
            .map_err(|e| e.to_string())?;
        let exts = [
            "java", "js", "ts", "tsx", "py", "kt", "rs", "go", "rb", "cls",
        ];
        let hits = search_source_files(&roots, &exts, &pattern, 60);

        if hits.is_empty() {
            return Ok(format!(
                "No usages of '{}' found across source roots.",
                class_name
            ));
        }
        let truncated = hits.len() >= 60;
        let mut lines = vec![format!(
            "Usages of {} ({}{}  matches):\n",
            class_name,
            hits.len(),
            if truncated { "+" } else { "" }
        )];
        let mut prev_file = String::new();
        for (rel, lineno, content) in &hits {
            if *rel != prev_file {
                lines.push(format!("  {}:", rel));
                prev_file = rel.clone();
            }
            lines.push(format!("    L{}: {}", lineno, content));
        }
        if truncated {
            lines.push("\n  (truncated at 60 matches — use a more specific name)".into());
        }
        Ok(lines.join("\n"))
    }

    fn tool_codesynapse_context(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let query = Self::str_arg(args, "query")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let max_chars = Self::int_arg(args, "max_chars", 16000) as usize;

        let g = self.load_module_serve_graph(graph_name)?;
        let dense = self
            .embedder
            .as_ref()
            .filter(|_| !self.node_embeddings.is_empty())
            .map(|e| (e, &self.node_embeddings));

        let result = context_query(&g, query, dense);

        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower.split_whitespace().collect();

        let mut out = String::new();
        let mut total_chars = 0usize;

        if result.fallback {
            out.push_str("[No exact match — showing semantic results]\n\n");
        }

        let render_group = |header: &str,
                            nodes: &[(String, String, String)],
                            out: &mut String,
                            total_chars: &mut usize,
                            max_chars: usize,
                            query_tokens: &[&str],
                            require_source: bool|
         -> (bool, usize) {
            if nodes.is_empty() {
                return (true, 0);
            }
            let mut rendered = 0usize;
            let mut header_written = false;
            for (_, label, source_file) in nodes {
                if *total_chars >= max_chars {
                    return (false, rendered);
                }
                let path = std::path::PathBuf::from(source_file);
                let source = self.get_source_cached(&path);
                if require_source && source.is_none() {
                    continue; // skip nodes whose source file is unreadable
                }
                if !header_written {
                    out.push_str(&format!("## {}\n", header));
                    header_written = true;
                }
                let header_line = format!(
                    "### {} ({})\n",
                    label,
                    path.file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(source_file)
                );
                out.push_str(&header_line);
                rendered += 1;

                if let Some((content, items)) = source {
                    let (content, items) = (content, items);
                    let source_lines: Vec<&str> = content.lines().collect();

                    // Find the best-matching method body
                    let mut scored: Vec<(usize, &OutlineItem)> = items
                        .iter()
                        .filter(|i| matches!(i.kind.as_str(), "method" | "function"))
                        .map(|item| {
                            let name_lower = item.name.to_lowercase();
                            let label_lower = label.to_lowercase();
                            let score = if name_lower == label_lower {
                                100
                            } else {
                                query_tokens
                                    .iter()
                                    .filter(|t| name_lower.contains(*t))
                                    .count()
                            };
                            (score, item)
                        })
                        .collect();
                    scored.sort_by_key(|k| std::cmp::Reverse(k.0));

                    let mut wrote_body = false;
                    for &(_, item) in &scored {
                        if let Some((start, end)) =
                            extract_method_range(&source_lines, &items, &item.name)
                        {
                            let body: Vec<String> = source_lines[start - 1..end]
                                .iter()
                                .enumerate()
                                .map(|(i, l)| format!("{:<6} {}", start + i, l))
                                .collect();
                            out.push_str(&body.join("\n"));
                            out.push('\n');
                            wrote_body = true;
                            break;
                        }
                    }
                    if !wrote_body {
                        let numbered: Vec<String> = content
                            .lines()
                            .enumerate()
                            .map(|(i, l)| format!("{:<6} {}", i + 1, l))
                            .collect();
                        let joined = numbered.join("\n");
                        let snippet: String = joined.chars().take(2048).collect();
                        out.push_str(&snippet);
                        out.push('\n');
                    } else if let Some(method_item) = scored.first().map(|(_, i)| *i) {
                        let method_line = method_item.line;
                        let parent_class = items
                            .iter()
                            .filter(|i| i.kind == "class" && i.line <= method_line)
                            .max_by_key(|i| i.line);
                        if let Some(cls) = parent_class {
                            let next_class_line = items
                                .iter()
                                .filter(|i| i.kind == "class" && i.line > cls.line)
                                .map(|i| i.line)
                                .min()
                                .unwrap_or(usize::MAX);
                            let sibling_count = items
                                .iter()
                                .filter(|i| {
                                    matches!(i.kind.as_str(), "method" | "function")
                                        && i.line > cls.line
                                        && i.line < next_class_line
                                        && i.name != method_item.name
                                })
                                .count();
                            if sibling_count > 0 {
                                out.push_str(&format!(
                                    "> {} has {} other method(s) not shown — \
                                    call codesynapse_read(\"{}\") to see the full class\n",
                                    cls.name, sibling_count, cls.name
                                ));
                            }
                        }
                    }
                } // end if let Some(source)
                out.push('\n');
                *total_chars = out.len();
            }
            (true, rendered)
        };

        type NodeGroup<'a> = (&'a str, &'a [(String, String, String)]);
        let groups: &[NodeGroup<'_>] = &[
            ("Entry Points", &result.entry_points),
            ("Callers", &result.callers),
            ("Callees", &result.callees),
        ];

        let mut entry_rendered = 0usize;
        let mut fell_back = result.fallback;
        for (header, nodes) in groups {
            let (completed, rendered) = render_group(
                header,
                nodes,
                &mut out,
                &mut total_chars,
                max_chars,
                &query_tokens,
                true, // require_source: skip nodes with unreadable paths
            );
            if *header == "Entry Points" {
                entry_rendered = rendered;
            }
            if !completed {
                out.push_str("\n[truncated — raise max_chars to see more]\n");
                break;
            }
        }

        // Exact match found but every matched node had an unreadable source path —
        // fall back to semantic so the response is not empty.
        if !result.fallback && entry_rendered == 0 && !result.entry_points.is_empty() {
            out.clear();
            out.push_str("[No exact match — showing semantic results]\n\n");
            let fallback_nodes = query_top_nodes(&g, query, 5, dense);
            render_group(
                "Entry Points",
                &fallback_nodes,
                &mut out,
                &mut total_chars,
                max_chars,
                &query_tokens,
                false, // don't require source for semantic fallback
            );
            fell_back = true;
        }

        if entry_rendered > 0 && result.callers.is_empty() && !fell_back {
            out.push_str(
                "\n> No callers found — symbol is defined but not called within this graph.\n",
            );
        }

        if out.trim().is_empty() || out.trim() == "[No exact match — showing semantic results]" {
            return Ok("No results found.".into());
        }

        if !fell_back {
            let node_count = g.num_nodes();
            out.push_str(&format!(
                "\n---\n> Exact match found (graph has {} indexed nodes). \
                The entry points and source above cover the relevant mechanism. \
                Answer from this context.\n",
                node_count
            ));
        }

        self.log_tool_call("codesynapse_context", out.len(), total_chars);
        Ok(out)
    }

    fn tool_codesynapse_resolve(
        &self,
        args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let query = Self::str_arg(args, "query")?;
        let graph_name = args
            .get("graph")
            .and_then(|v| v.as_str())
            .unwrap_or("merged");
        let top_k = Self::int_arg(args, "top_k", 3) as usize;
        let max_chars = Self::int_arg(args, "max_chars", 24000) as usize;

        let g = self.load_module_serve_graph(graph_name)?;
        let dense = self
            .embedder
            .as_ref()
            .filter(|_| !self.node_embeddings.is_empty())
            .map(|e| (e, &self.node_embeddings));

        let top_nodes = query_top_nodes(&g, query, top_k, dense);
        if top_nodes.is_empty() {
            return Ok("No matching nodes found.".into());
        }

        let low_confidence = top_nodes.len() > 1 && {
            let first_file = &top_nodes[0].2;
            top_nodes.iter().all(|(_, _, f)| f == first_file)
        };

        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower.split_whitespace().collect();

        let mut sections: Vec<String> = Vec::new();
        let mut total_chars = 0usize;
        let mut raw_chars = 0usize;

        for (node_id, label, source_file) in &top_nodes {
            if total_chars >= max_chars {
                break;
            }
            let path = PathBuf::from(source_file);
            let (content, items) = match self.get_source_cached(&path) {
                Some(v) => v,
                None => continue,
            };
            raw_chars += content.len();
            let source_lines: Vec<&str> = content.lines().collect();

            let mut scored_methods: Vec<(usize, &OutlineItem)> = items
                .iter()
                .filter(|i| i.kind == "method")
                .map(|item| {
                    let name_lower = item.name.to_lowercase();
                    let score = query_tokens
                        .iter()
                        .filter(|t| name_lower.contains(*t))
                        .count();
                    (score, item)
                })
                .collect();
            scored_methods.sort_by_key(|k| std::cmp::Reverse(k.0));

            let caller_count = g
                .edges_iter()
                .filter(|e| e.target == *node_id && e.relation.contains("call"))
                .count();
            let caller_note = if caller_count == 0 {
                " [0 explicit callers — may be entry point, registered callback, or unused]"
                    .to_string()
            } else {
                format!(" [{} caller(s)]", caller_count)
            };
            let mut sec = format!("═══ {} ({}){}\n", label, path.display(), caller_note);

            for item in &items {
                let prefix = match item.kind.as_str() {
                    "class" | "interface" | "enum" | "record" => "CLASS",
                    "method" => "  def",
                    "field" => "  var",
                    _ => "     ",
                };
                sec.push_str(&format!("  L{:<6} {} {}\n", item.line, prefix, item.name));
            }
            sec.push('\n');

            for &(score, item) in &scored_methods {
                if total_chars + sec.len() >= max_chars {
                    break;
                }
                if let Some((start, end)) = extract_method_range(&source_lines, &items, &item.name)
                {
                    let body: Vec<String> = source_lines[start - 1..end]
                        .iter()
                        .enumerate()
                        .map(|(i, l)| format!("{:<6} {}", start + i, l))
                        .collect();
                    let method_block = format!(
                        "── {}() [L{}-{}] {}\n{}\n",
                        item.name,
                        start,
                        end,
                        if score > 0 { "★" } else { "" },
                        body.join("\n")
                    );
                    if method_block.len() > 4000 && score == 0 {
                        continue;
                    }
                    sec.push_str(&method_block);
                }
            }

            total_chars += sec.len();
            sections.push(sec);
        }

        let mut result = sections.join("\n");
        if low_confidence {
            result = format!(
                "[low confidence — results are from one file; try rephrasing or use Bash to grep]\n\n{}",
                result
            );
        }
        self.log_tool_call(
            "codesynapse_resolve",
            result.len(),
            raw_chars.saturating_sub(result.len()),
        );
        Ok(result)
    }

    fn tool_graph_build(&self, _args: &Map<String, Value>) -> std::result::Result<String, String> {
        *self.last_mtime.lock().unwrap() = None;
        self.graph_cache.lock().unwrap().clear();
        let merged = self.global_dir.join("global-graph.json");
        if !merged.exists() {
            return Ok(
                "global-graph.json not found. Run `codesynapse global add <path>` to register graphs.".into()
            );
        }
        let size_kb = std::fs::metadata(&merged)
            .map(|m| m.len() / 1024)
            .unwrap_or(0);
        let manifest_path = self.global_dir.join("global-manifest.json");
        let n_modules = if manifest_path.exists() {
            std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("repos").and_then(|r| r.as_object()).map(|m| m.len()))
                .unwrap_or(0)
        } else {
            0
        };
        Ok(format!(
            "Graph cache cleared. Next query will reload from disk.\n  global-graph.json: {}KB\n  manifest: {} module(s)",
            size_kb, n_modules
        ))
    }

    fn tool_codesynapse_stats(
        &self,
        _args: &Map<String, Value>,
    ) -> std::result::Result<String, String> {
        let path = self.global_dir.join("tool_stats.jsonl");
        if !path.exists() {
            return Ok(
                "No usage data yet — stats are recorded as you use codesynapse tools.".into(),
            );
        }
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let entries: Vec<serde_json::Value> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        if entries.is_empty() {
            return Ok("No usage data yet.".into());
        }

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let sum_saved = |subset: &[&serde_json::Value]| -> f64 {
            subset
                .iter()
                .map(|e| e.get("saved_chars").and_then(|v| v.as_f64()).unwrap_or(0.0))
                .sum::<f64>()
                / 4.0
        };
        let fmt_tok = |n: f64| -> String {
            if n >= 1_000_000.0 {
                format!("{:.1}M", n / 1_000_000.0)
            } else if n >= 1_000.0 {
                format!("{:.1}k", n / 1_000.0)
            } else {
                format!("{:.0}", n)
            }
        };
        // bar width = 22; row total = 1+1+8+1+1+1+4+7+1+1+8+8+1+1+22+1+1 = 68 ✓
        let bar = |value: f64, max: f64| -> String {
            if max <= 0.0 {
                return "░".repeat(22);
            }
            let filled = ((value / max) * 22.0) as usize;
            let filled = filled.min(22);
            format!("{}{}", "█".repeat(filled), "░".repeat(22 - filled))
        };

        let today: Vec<&serde_json::Value> = entries
            .iter()
            .filter(|e| now - e.get("ts").and_then(|v| v.as_f64()).unwrap_or(0.0) < 86400.0)
            .collect();
        let week: Vec<&serde_json::Value> = entries
            .iter()
            .filter(|e| now - e.get("ts").and_then(|v| v.as_f64()).unwrap_or(0.0) < 7.0 * 86400.0)
            .collect();
        let all: Vec<&serde_json::Value> = entries.iter().collect();

        let today_saved = sum_saved(&today);
        let week_saved = sum_saved(&week);
        let total_saved = sum_saved(&all);
        let max_saved = today_saved.max(week_saved).max(total_saved).max(1.0);

        // Box: 68 chars total, 66 inner.
        // Row: ║ {:<8} │ {:>4} calls │ {:>8} tokens │ {bar_22} ║
        //       1 1  8  1 1 1  4    7 1 1  8      8  1 1  22  1 1 = 68 ✓
        // │ at inner positions 10, 23, 41 → ╪ at same positions in col_div
        let row = |label: &str, count: usize, saved: f64| -> String {
            format!(
                "║ {:<8} │ {:>4} calls │ {:>8} tokens │ {} ║",
                label,
                count,
                fmt_tok(saved),
                bar(saved, max_saved)
            )
        };

        // Top 5 tools by call count
        let mut tool_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for e in &entries {
            if let Some(t) = e.get("tool").and_then(|v| v.as_str()) {
                *tool_counts.entry(t.to_string()).or_insert(0) += 1;
            }
        }
        let mut ranked: Vec<(String, usize)> = tool_counts.into_iter().collect();
        ranked.sort_by_key(|b| std::cmp::Reverse(b.1));

        let top = "╔══════════════════════════════════════════════════════════════════╗";
        // col_div: ╪ at inner positions 10, 23, 41 — aligns with │ in data rows
        let col_div = "╠══════════╪════════════╪═════════════════╪════════════════════════╣";
        let full_div = "╠══════════════════════════════════════════════════════════════════╣";
        let bot = "╚══════════════════════════════════════════════════════════════════╝";

        let mut lines = vec![
            String::new(),
            top.into(),
            format!("║{:^66}║", " codesynapse -- usage stats "),
            col_div.into(),
            row("Today", today.len(), today_saved),
            row("Last 7d", week.len(), week_saved),
            row("All time", entries.len(), total_saved),
        ];

        if !ranked.is_empty() {
            lines.push(full_div.into());
            lines.push(format!("║{:^66}║", " top tools "));
            lines.push(full_div.into());
            for (i, (tool, count)) in ranked.iter().take(5).enumerate() {
                let short = tool.strip_prefix("codesynapse_").unwrap_or(tool);
                let short = if short.len() > 55 {
                    &short[..55]
                } else {
                    short
                };
                let label = format!("  {}. {}", i + 1, short);
                // right-align count in last 6 chars; total inner = 60+6 = 66 ✓
                let content = format!("{:<60}{:>6}", label, count);
                lines.push(format!("║{}║", content));
            }
        }

        lines.push(bot.into());
        lines.push(String::new());
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesynapse_core::graph::MemoryGraphStore;
    use codesynapse_core::types::{Edge, Node};
    use serde_json::Map;

    fn setup_store() -> StoreBackend {
        let store = MemoryGraphStore::new();
        let backend = StoreBackend::Memory(store);
        let nodes = vec![
            node("a", "NodeA"),
            node("b", "NodeB"),
            node("c", "NodeC"),
            node("d", "NodeD"),
        ];
        for n in &nodes {
            backend.add_node(n.clone()).unwrap();
        }
        for e in &[
            edge("a", "b", "calls"),
            edge("b", "c", "calls"),
            edge("c", "d", "calls"),
        ] {
            backend.add_edge(e.clone()).unwrap();
        }
        backend
    }

    fn make_server(backend: StoreBackend) -> McpServer {
        let global_dir = PathBuf::from("/nonexistent-test-global");
        McpServer {
            backend: Mutex::new(backend),
            graph_path: None,
            last_mtime: Mutex::new(None),
            telemetry: Arc::new(codesynapse_core::telemetry::Telemetry::new(
                global_dir.clone(),
            )),
            global_dir,
            embedder: None,
            node_embeddings: HashMap::new(),
            graph_cache: Mutex::new(HashMap::new()),
            source_cache: Mutex::new(HashMap::new()),
            stale_check: Mutex::new(None),
        }
    }

    fn make_server_with_global(backend: StoreBackend, global_dir: PathBuf) -> McpServer {
        McpServer {
            backend: Mutex::new(backend),
            graph_path: None,
            last_mtime: Mutex::new(None),
            telemetry: Arc::new(codesynapse_core::telemetry::Telemetry::new(
                global_dir.clone(),
            )),
            global_dir,
            embedder: None,
            node_embeddings: HashMap::new(),
            graph_cache: Mutex::new(HashMap::new()),
            source_cache: Mutex::new(HashMap::new()),
            stale_check: Mutex::new(None),
        }
    }

    fn node(id: &str, label: &str) -> Node {
        Node {
            id: id.to_string(),
            label: label.to_string(),
            file_type: "code".to_string(),
            source_file: "test.rs".to_string(),
            source_location: None,
            community: None,
            rationale: None,
            docstring: None,
            metadata: HashMap::new(),
        }
    }

    fn edge(src: &str, tgt: &str, rel: &str) -> Edge {
        Edge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: rel.to_string(),
            confidence: "EXTRACTED".to_string(),
            source_file: Some("test.rs".to_string()),
            weight: 1.0,
            context: None,
        }
    }

    #[test]
    fn test_tools_list() {
        let tools = tool_defs();
        assert_eq!(tools.len(), 33);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"codesynapse_context"));
        assert!(names.contains(&"codesynapse_query_graph"));
        assert!(names.contains(&"codesynapse_get_node"));
        assert!(names.contains(&"codesynapse_get_neighbors"));
        assert!(names.contains(&"codesynapse_get_community"));
        assert!(names.contains(&"codesynapse_god_nodes"));
        assert!(names.contains(&"codesynapse_graph_stats"));
        assert!(names.contains(&"codesynapse_shortest_path"));
        assert!(names.contains(&"codesynapse_find_all_paths"));
        assert!(names.contains(&"codesynapse_weighted_path"));
        assert!(names.contains(&"codesynapse_community_bridges"));
        assert!(names.contains(&"codesynapse_diff"));
        assert!(names.contains(&"codesynapse_pagerank"));
        assert!(names.contains(&"codesynapse_detect_cycles"));
        assert!(names.contains(&"codesynapse_smart_summary"));
        assert!(names.contains(&"codesynapse_find_similar"));
        assert!(names.contains(&"codesynapse_query_vector"));
        assert!(names.contains(&"codesynapse_blast_radius"));
        assert!(names.contains(&"codesynapse_blast_radius_scored"));
        assert!(names.contains(&"codesynapse_blast_radius_multi"));
        assert!(names.contains(&"codesynapse_query_semantic"));
        assert!(names.contains(&"codesynapse_hierarchy"));
        assert!(names.contains(&"codesynapse_list_graphs"));
        assert!(names.contains(&"codesynapse_module_summary"));
        assert!(names.contains(&"codesynapse_outline"));
        assert!(names.contains(&"codesynapse_read"));
        assert!(names.contains(&"codesynapse_read_method"));
        assert!(names.contains(&"codesynapse_read_with_callees"));
        assert!(names.contains(&"codesynapse_find_callers"));
        assert!(names.contains(&"codesynapse_find_usages"));
        assert!(names.contains(&"codesynapse_build"));
        assert!(names.contains(&"codesynapse_stats"));
    }

    #[test]
    fn test_tools_list_json() {
        let server = make_server(setup_store());
        let request = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "tools/list".into(),
            params: None,
        };
        let response = server.handle_request(&request);
        assert_eq!(response.id, Some(serde_json::json!(1)));
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 33);
    }

    #[test]
    fn test_tool_graph_stats() {
        let server = make_server(setup_store());
        let result = server.tool_graph_stats(&Map::new()).unwrap();
        assert!(result.contains("Nodes: 4"));
        assert!(result.contains("Edges: 3"));
    }

    #[test]
    fn test_tool_get_node_found() {
        let server = make_server(setup_store());
        let mut args = Map::new();
        args.insert("node_id".to_string(), serde_json::json!("a"));
        let result = server.tool_get_node(&args).unwrap();
        assert!(result.contains("NodeA"));
    }

    #[test]
    fn test_tool_get_node_not_found() {
        let server = make_server(setup_store());
        let mut args = Map::new();
        args.insert("node_id".to_string(), serde_json::json!("z"));
        let result = server.tool_get_node(&args).unwrap();
        assert!(result.contains("not found"));
    }

    #[test]
    fn test_tool_god_nodes() {
        let server = make_server(setup_store());
        let mut args = Map::new();
        args.insert("top_n".to_string(), serde_json::json!(3));
        let result = server.tool_god_nodes(&args).unwrap();
        assert!(result.contains("god node"));
    }

    #[test]
    fn test_tool_shortest_path() {
        let server = make_server(setup_store());
        let mut args = Map::new();
        args.insert("source".to_string(), serde_json::json!("a"));
        args.insert("target".to_string(), serde_json::json!("d"));
        let result = server.tool_shortest_path(&args).unwrap();
        assert!(result.contains("NodeA"));
        assert!(result.contains("NodeD"));
    }

    #[test]
    fn test_tool_find_all_paths() {
        let server = make_server(setup_store());
        let mut args = Map::new();
        args.insert("source".to_string(), serde_json::json!("a"));
        args.insert("target".to_string(), serde_json::json!("d"));
        args.insert("max_length".to_string(), serde_json::json!(5));
        let result = server.tool_find_all_paths(&args).unwrap();
        assert!(result.contains("a -> b"));
    }

    #[test]
    fn test_tool_smart_summary_architecture() {
        let server = make_server(setup_store());
        let mut args = Map::new();
        args.insert("level".to_string(), serde_json::json!("architecture"));
        let result = server.tool_smart_summary(&args).unwrap();
        assert!(result.contains("Nodes: 4"));
        assert!(result.contains("Edges: 3"));
    }

    #[test]
    fn test_tool_unknown_method() {
        let server = make_server(setup_store());
        let request = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "unknown".into(),
            params: None,
        };
        let response = server.handle_request(&request);
        assert!(response.error.is_some());
    }

    #[test]
    fn test_tool_unknown_tool() {
        let server = make_server(setup_store());
        let mut args = Map::new();
        args.insert("name".to_string(), serde_json::json!("nonexistent"));
        args.insert("arguments".to_string(), serde_json::json!({}));
        let params = serde_json::json!({"name": "nonexistent", "arguments": {}});
        let request = JsonRpcRequest {
            id: Some(serde_json::json!(1)),
            method: "tools/call".into(),
            params: Some(params),
        };
        let response = server.handle_request(&request);
        assert!(response.error.is_some());
    }

    #[test]
    fn test_server_hotreload_detects_mtime_change() {
        use std::io::Cursor;
        use std::time::Duration;

        let graph_file = std::env::temp_dir().join("codesynapse_hotreload_test.tmp");
        std::fs::write(&graph_file, b"placeholder").unwrap();

        let global_dir = PathBuf::from("/nonexistent-test-global");
        let server = McpServer {
            backend: Mutex::new(setup_store()),
            graph_path: Some(graph_file.clone()),
            last_mtime: Mutex::new(None),
            telemetry: Arc::new(codesynapse_core::telemetry::Telemetry::new(
                global_dir.clone(),
            )),
            global_dir,
            embedder: None,
            node_embeddings: HashMap::new(),
            graph_cache: Mutex::new(HashMap::new()),
            source_cache: Mutex::new(HashMap::new()),
            stale_check: Mutex::new(None),
        };

        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n";
        server
            .run_on(Cursor::new(input.as_bytes()), Vec::new())
            .unwrap();
        let mtime_after_first = *server.last_mtime.lock().unwrap();
        assert!(
            mtime_after_first.is_some(),
            "mtime should be recorded after first call"
        );

        // Simulate mtime change: reset recorded mtime to an old value
        let old_mtime = mtime_after_first.unwrap() - Duration::from_secs(10);
        *server.last_mtime.lock().unwrap() = Some(old_mtime);

        // Touch the file so its real mtime differs from what we injected
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&graph_file, b"updated").unwrap();

        server
            .run_on(Cursor::new(input.as_bytes()), Vec::new())
            .unwrap();
        let mtime_after_second = *server.last_mtime.lock().unwrap();
        assert_ne!(
            Some(old_mtime),
            mtime_after_second,
            "mtime should be refreshed after file change"
        );

        let _ = std::fs::remove_file(&graph_file);
    }

    #[test]
    fn test_blank_stdin_lines_do_not_crash() {
        use std::io::Cursor;
        let server = make_server(setup_store());
        // Mix of blank, whitespace-only, and a valid request
        let input = "\n   \n\t\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n\n";
        let reader = Cursor::new(input.as_bytes());
        let mut output = Vec::new();
        server
            .run_on(reader, &mut output)
            .expect("should not crash on blank lines");
        let out_str = String::from_utf8(output).unwrap();
        assert!(
            !out_str.is_empty(),
            "should produce response for valid line"
        );
        assert!(
            out_str.contains("tools"),
            "response should include tool listing"
        );
        // Only one JSON object should appear (blank lines produce no output)
        assert_eq!(out_str.lines().count(), 1);
    }

    // ---------------------------------------------------------------------------
    // Phase 2 — Graph MCP tool tests

    fn write_test_graph(
        dir: &std::path::Path,
        name: &str,
        nodes: &[(&str, &str, &str)],
        edges: &[(&str, &str, &str)],
    ) -> PathBuf {
        let nodes_json: Vec<serde_json::Value> = nodes
            .iter()
            .map(|(id, label, ft)| serde_json::json!({"id": id, "label": label, "file_type": ft, "source_file": "src/x.rs"}))
            .collect();
        let edges_json: Vec<serde_json::Value> = edges
            .iter()
            .map(|(src, tgt, rel)| serde_json::json!({"source": src, "target": tgt, "relation": rel, "confidence": "EXTRACTED"}))
            .collect();
        let path = dir.join(name);
        std::fs::write(
            &path,
            serde_json::to_string(&serde_json::json!({"nodes": nodes_json, "edges": edges_json}))
                .unwrap(),
        )
        .unwrap();
        path
    }

    fn write_manifest(global_dir: &std::path::Path, entries: &[(&str, &str, usize, usize)]) {
        use codesynapse_core::global_graph::{GlobalManifest, RepoManifestEntry};
        let mut manifest = GlobalManifest::default();
        for (tag, path, node_count, edge_count) in entries {
            manifest.repos.insert(
                tag.to_string(),
                RepoManifestEntry {
                    added_at: "2026-01-01T00:00:00Z".into(),
                    source_path: path.to_string(),
                    node_count: *node_count,
                    edge_count: *edge_count,
                    source_hash: "abc123".into(),
                },
            );
        }
        std::fs::create_dir_all(global_dir).unwrap();
        std::fs::write(
            global_dir.join("global-manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn test_graph_list_graphs_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let result = server.tool_graph_list_graphs(&Map::new()).unwrap();
        assert!(result.contains("No graphs registered"));
    }

    #[test]
    fn test_graph_list_graphs_with_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let graph_path = write_test_graph(tmp.path(), "g.json", &[("a", "MyClass", "code")], &[]);
        write_manifest(
            tmp.path(),
            &[("mymodule", graph_path.to_str().unwrap(), 1, 0)],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let result = server.tool_graph_list_graphs(&Map::new()).unwrap();
        assert!(result.contains("mymodule"));
        assert!(result.contains("1"));
    }

    #[test]
    fn test_graph_module_summary_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("module".into(), serde_json::json!("nonexistent"));
        let result = server.tool_graph_module_summary(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_graph_module_summary_found() {
        let tmp = tempfile::tempdir().unwrap();
        let graph_path = write_test_graph(
            tmp.path(),
            "g.json",
            &[("a", "Alpha", "code"), ("b", "Beta", "code")],
            &[("a", "b", "calls")],
        );
        write_manifest(tmp.path(), &[("mymod", graph_path.to_str().unwrap(), 2, 1)]);
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("module".into(), serde_json::json!("mymod"));
        let result = server.tool_graph_module_summary(&args).unwrap();
        assert!(result.contains("mymod"));
        assert!(result.contains("Nodes:"));
    }

    #[test]
    fn test_graph_blast_radius() {
        let tmp = tempfile::tempdir().unwrap();
        // A→B→C, D is disconnected
        let graph_path = write_test_graph(
            tmp.path(),
            "g.json",
            &[
                ("a", "Alpha", "code"),
                ("b", "Beta", "code"),
                ("c", "Gamma", "code"),
                ("d", "Delta", "code"),
            ],
            &[("a", "b", "calls"), ("b", "c", "calls")],
        );
        // Use as "merged" (global-graph.json)
        std::fs::copy(&graph_path, tmp.path().join("global-graph.json")).unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("Alpha"));
        args.insert("depth".into(), serde_json::json!(3));
        let result = server.tool_graph_blast_radius(&args).unwrap();
        assert!(result.contains("Alpha"));
        assert!(result.contains("Beta") || result.contains("b"));
    }

    #[test]
    fn test_graph_blast_radius_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Alpha", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("ZZZNotHere"));
        let result = server.tool_graph_blast_radius(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_graph_hierarchy_no_edges() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Animal", "code"), ("b", "Dog", "code")],
            &[("b", "a", "extends")],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("Animal"));
        let result = server.tool_graph_hierarchy(&args).unwrap();
        assert!(result.contains("Animal"));
        assert!(result.contains("Dog") || result.contains("b"));
    }

    #[test]
    fn test_graph_query_vector_returns_result() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "UserService", "code"), ("b", "AuthService", "code")],
            &[("a", "b", "calls")],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("query".into(), serde_json::json!("user"));
        let result = server.tool_graph_query_vector(&args).unwrap();
        assert!(!result.is_empty());
    }

    // ---------------------------------------------------------------------------
    // Phase 3 & 4 tests

    fn write_source_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn write_graph_with_abs_source(
        tmp: &std::path::Path,
        graph_name: &str,
        src_abs: &str,
    ) -> PathBuf {
        let json = serde_json::json!({
            "nodes": [{"id": "cls1", "label": "MyClass", "file_type": "code", "source_file": src_abs}],
            "edges": []
        });
        let path = tmp.join(graph_name);
        std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
        path
    }

    #[test]
    fn test_outline_java_basic() {
        let path = PathBuf::from("Foo.java");
        let content = "public class Foo {\n  public void bar() {\n  }\n}";
        let items = outline_items(&path, content);
        let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"class"), "should find class");
        assert!(kinds.contains(&"method"), "should find method");
    }

    #[test]
    fn test_outline_python_basic() {
        let path = PathBuf::from("foo.py");
        let content = "class Foo:\n    def bar(self):\n        pass\n\ndef top_func():\n    pass";
        let items = outline_items(&path, content);
        assert!(items.iter().any(|i| i.kind == "class" && i.name == "Foo"));
        assert!(items.iter().any(|i| i.kind == "method" && i.name == "bar"));
        assert!(items
            .iter()
            .any(|i| i.kind == "method" && i.name == "top_func"));
    }

    #[test]
    fn test_outline_js_basic() {
        let path = PathBuf::from("foo.ts");
        let content = "export class Foo {\n  doThing() {\n  }\n}";
        let items = outline_items(&path, content);
        assert!(items.iter().any(|i| i.kind == "class" && i.name == "Foo"));
        assert!(items
            .iter()
            .any(|i| i.kind == "method" && i.name == "doThing"));
    }

    #[test]
    fn test_detect_method_end_brace_tracking() {
        let lines: Vec<&str> = vec!["void foo() {", "  if (x) { bar(); }", "}", "void next() {"];
        let end = detect_method_end(&lines, 0);
        assert_eq!(end, 2);
    }

    #[test]
    fn test_extract_method_range_found() {
        let lines: Vec<&str> = vec!["public void doIt() {", "  x++;", "}"];
        let items = vec![OutlineItem {
            kind: "method".into(),
            name: "doIt".into(),
            line: 1,
        }];
        let result = extract_method_range(&lines, &items, "doIt");
        assert!(result.is_some());
        let (s, e) = result.unwrap();
        assert_eq!(s, 1);
        assert_eq!(e, 3);
    }

    #[test]
    fn test_source_roots_from_graph_empty() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Foo", "code")],
            &[],
        );
        let g = load_graph(&tmp.path().join("global-graph.json")).unwrap();
        let roots = source_roots_from_graph(&g);
        assert!(
            roots.is_empty(),
            "relative source_file paths should yield no roots"
        );
    }

    #[test]
    fn test_synapse_outline_resolves_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_source_file(
            tmp.path(),
            "Foo.java",
            "public class Foo {\n  public void bar() {}\n}",
        );
        write_graph_with_abs_source(tmp.path(), "global-graph.json", src.to_str().unwrap());
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("MyClass"));
        let result = server.tool_codesynapse_outline(&args).unwrap();
        assert!(
            result.contains("Foo.java") || result.contains("File:"),
            "should include file info"
        );
    }

    #[test]
    fn test_synapse_outline_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Foo", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("ZZZNotHere"));
        let result = server.tool_codesynapse_outline(&args);
        assert!(result.is_err() || result.unwrap().contains("not found"));
    }

    #[test]
    fn test_synapse_read_line_range() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_source_file(tmp.path(), "Bar.java", "line1\nline2\nline3\nline4\nline5");
        write_graph_with_abs_source(tmp.path(), "global-graph.json", src.to_str().unwrap());
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("MyClass"));
        args.insert("from_line".into(), serde_json::json!(2));
        args.insert("to_line".into(), serde_json::json!(3));
        let result = server.tool_codesynapse_read(&args).unwrap();
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
        assert!(!result.contains("line4"));
    }

    #[test]
    fn test_synapse_read_method_found() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_source_file(
            tmp.path(),
            "Svc.java",
            "public class Svc {\n  public void doWork() {\n    int x = 1;\n  }\n}",
        );
        write_graph_with_abs_source(tmp.path(), "global-graph.json", src.to_str().unwrap());
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("MyClass"));
        args.insert("method_name".into(), serde_json::json!("doWork"));
        let result = server.tool_codesynapse_read_method(&args).unwrap();
        assert!(result.contains("doWork"));
        assert!(result.contains("int x = 1"));
    }

    #[test]
    fn test_synapse_read_method_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_source_file(
            tmp.path(),
            "Svc.java",
            "public class Svc {\n  public void doWork() {}\n}",
        );
        write_graph_with_abs_source(tmp.path(), "global-graph.json", src.to_str().unwrap());
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("MyClass"));
        args.insert("method_name".into(), serde_json::json!("nonexistentMethod"));
        let result = server.tool_codesynapse_read_method(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No method matching"));
    }

    #[test]
    fn test_synapse_read_with_callees() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_source_file(tmp.path(), "Svc.java",
            "public class Svc {\n  public void main() {\n    helper();\n  }\n  public void helper() {\n    int x = 0;\n  }\n}");
        write_graph_with_abs_source(tmp.path(), "global-graph.json", src.to_str().unwrap());
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("MyClass"));
        args.insert("method_name".into(), serde_json::json!("main"));
        let result = server.tool_codesynapse_read_with_callees(&args).unwrap();
        assert!(result.contains("main"));
        assert!(result.contains("helper"));
    }

    #[test]
    fn test_synapse_find_callers_no_graph_edges() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Foo", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("Foo"));
        let result = server.tool_codesynapse_find_callers(&args).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_synapse_find_callers_via_graph_edges() {
        let tmp = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "nodes": [
                {"id": "a", "label": "FooService", "file_type": "code", "source_file": "Foo.java"},
                {"id": "b", "label": "BarController", "file_type": "code", "source_file": "Bar.java"}
            ],
            "edges": [
                {"source": "b", "target": "a", "relation": "calls", "confidence": "EXTRACTED"}
            ]
        });
        std::fs::write(
            tmp.path().join("global-graph.json"),
            serde_json::to_string(&json).unwrap(),
        )
        .unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("FooService"));
        let result = server.tool_codesynapse_find_callers(&args).unwrap();
        assert!(result.contains("BarController") || result.contains("graph call edges"));
    }

    #[test]
    fn test_synapse_find_usages_no_roots() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Foo", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("Foo"));
        let result = server.tool_codesynapse_find_usages(&args).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_graph_build_no_graph() {
        let tmp = tempfile::tempdir().unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let result = server.tool_graph_build(&Map::new()).unwrap();
        assert!(result.contains("not found") || result.contains("global-graph"));
    }

    #[test]
    fn test_graph_build_with_graph() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Foo", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let result = server.tool_graph_build(&Map::new()).unwrap();
        assert!(result.contains("cleared") || result.contains("reload"));
    }

    #[test]
    fn test_synapse_stats_no_data() {
        let tmp = tempfile::tempdir().unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let result = server.tool_codesynapse_stats(&Map::new()).unwrap();
        assert!(result.contains("No usage data"));
    }

    #[test]
    fn test_synapse_stats_with_data() {
        let tmp = tempfile::tempdir().unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        // Log some fake stats
        server.log_tool_call("codesynapse_outline", 100, 5000);
        server.log_tool_call("codesynapse_read_method", 200, 8000);
        let result = server.tool_codesynapse_stats(&Map::new()).unwrap();
        assert!(result.contains("codesynapse"));
        assert!(result.contains("calls"));
    }

    #[test]
    fn test_tool_blast_radius_scored_ranks_high_in_degree_first() {
        let tmp = tempfile::tempdir().unwrap();
        // Alpha→Beta, plus 10 other nodes also pointing to Beta → high in-degree for Beta
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[
                ("a", "Alpha", "code"),
                ("b", "Beta", "code"),
                ("c", "C1", "code"),
                ("d", "C2", "code"),
                ("e", "C3", "code"),
                ("f", "C4", "code"),
                ("g", "C5", "code"),
                ("h", "C6", "code"),
                ("i", "C7", "code"),
                ("j", "C8", "code"),
                ("k", "C9", "code"),
                ("l", "C10", "code"),
            ],
            &[
                ("a", "b", "calls"),
                ("c", "b", "calls"),
                ("d", "b", "calls"),
                ("e", "b", "calls"),
                ("f", "b", "calls"),
                ("g", "b", "calls"),
                ("h", "b", "calls"),
                ("i", "b", "calls"),
                ("j", "b", "calls"),
                ("k", "b", "calls"),
                ("l", "b", "calls"),
            ],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("Alpha"));
        args.insert("depth".into(), serde_json::json!(2));
        let result = server.tool_graph_blast_radius_scored(&args).unwrap();
        assert!(result.contains("Beta"));
        assert!(result.contains("HIGH") || result.contains("MEDIUM"));
    }

    #[test]
    fn test_tool_blast_radius_scored_no_node() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Alpha", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_name".into(), serde_json::json!("ZZZGhost"));
        let result = server.tool_graph_blast_radius_scored(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_blast_radius_multi_union() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[
                ("a", "Alpha", "code"),
                ("b", "Beta", "code"),
                ("c", "Gamma", "code"),
                ("d", "Delta", "code"),
            ],
            &[("a", "c", "calls"), ("b", "d", "calls")],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("class_names".into(), serde_json::json!(["Alpha", "Beta"]));
        let result = server.tool_graph_blast_radius_multi(&args).unwrap();
        assert!(result.contains("Alpha") || result.contains("a"));
        assert!(result.contains("Beta") || result.contains("b"));
        assert!(result.contains("Gamma") || result.contains("Delta") || result.contains("Total"));
    }

    #[test]
    fn test_tool_blast_radius_multi_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Alpha", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert(
            "class_names".into(),
            serde_json::json!(["Alpha", "ZZZGhost"]),
        );
        let result = server.tool_graph_blast_radius_multi(&args).unwrap();
        assert!(result.contains("ZZZGhost"));
        assert!(result.contains("Not found"));
    }

    #[test]
    fn test_tool_graph_query_semantic_no_semantic_edges() {
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "UserService", "code"), ("b", "AuthService", "code")],
            &[("a", "b", "calls")],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("query".into(), serde_json::json!("user"));
        let result = server.tool_graph_query_semantic(&args).unwrap();
        assert!(result.contains("--llm") || result.contains("semantic"));
    }

    #[test]
    fn test_find_nodes_by_label_exact_first() {
        let tmp = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "nodes": [
                {"id": "a", "label": "FooService", "file_type": "code", "source_file": ""},
                {"id": "b", "label": "Foo", "file_type": "code", "source_file": ""}
            ],
            "edges": []
        });
        std::fs::write(
            tmp.path().join("global-graph.json"),
            serde_json::to_string(&json).unwrap(),
        )
        .unwrap();
        let g = load_graph(&tmp.path().join("global-graph.json")).unwrap();
        let results = find_nodes_by_label(&g, "Foo");
        assert!(!results.is_empty());
        assert_eq!(results[0].1.label, "Foo", "exact match should be first");
    }

    #[test]
    fn test_outline_rust() {
        let src = r#"
pub struct Runtime { handle: Handle }
pub trait Scheduler: Send { fn schedule(&self); }
impl Runtime {
    pub fn new() -> Self { todo!() }
    pub async fn block_on<F>(&self, f: F) {}
}
impl Scheduler for Runtime {
    fn schedule(&self) {}
}
async fn spawn_local<F>(f: F) {}
fn park() {}
"#;
        let items = outline_rust(src);
        let kinds: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.kind.as_str(), i.name.as_str()))
            .collect();
        assert!(kinds.contains(&("class", "Runtime")), "struct→class");
        assert!(
            kinds.contains(&("interface", "Scheduler")),
            "trait→interface"
        );
        assert!(
            kinds.iter().any(|(k, n)| *k == "method" && *n == "new"),
            "fn new"
        );
        assert!(
            kinds
                .iter()
                .any(|(k, n)| *k == "method" && *n == "block_on"),
            "async fn"
        );
        assert!(
            kinds
                .iter()
                .any(|(k, n)| *k == "method" && *n == "spawn_local"),
            "top-level async fn"
        );
        assert!(
            kinds.iter().any(|(k, n)| *k == "method" && *n == "park"),
            "top-level fn"
        );
        // impl Scheduler for Runtime → name=Runtime
        assert!(
            kinds
                .iter()
                .filter(|(k, n)| *k == "class" && *n == "Runtime")
                .count()
                >= 2,
            "impl+struct both captured"
        );
    }

    #[test]
    fn test_outline_go() {
        let src = r#"
type Server struct { port int }
type Handler interface { ServeHTTP() }
func NewServer() *Server { return nil }
func (s *Server) Start() {}
"#;
        let items = outline_go(src);
        let kinds: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.kind.as_str(), i.name.as_str()))
            .collect();
        assert!(kinds.contains(&("class", "Server")));
        assert!(kinds.contains(&("interface", "Handler")));
        assert!(kinds
            .iter()
            .any(|(k, n)| *k == "method" && *n == "NewServer"));
        assert!(kinds.iter().any(|(k, n)| *k == "method" && *n == "Start"));
    }

    #[test]
    fn test_outline_ruby() {
        let src = r#"
class User
  def initialize(name); end
  def self.find(id); end
end
module Auth
  def authenticate; end
end
"#;
        let items = outline_ruby(src);
        let kinds: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.kind.as_str(), i.name.as_str()))
            .collect();
        assert!(kinds.contains(&("class", "User")));
        assert!(kinds.contains(&("class", "Auth")));
        assert!(kinds
            .iter()
            .any(|(k, n)| *k == "method" && *n == "initialize"));
        assert!(kinds.iter().any(|(k, n)| *k == "method" && *n == "find"));
    }

    #[test]
    fn test_outline_elixir() {
        let src = r#"
defmodule MyApp.Router do
  def call(conn, opts), do: conn
  defp handle(conn), do: conn
end
"#;
        let items = outline_elixir(src);
        let kinds: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.kind.as_str(), i.name.as_str()))
            .collect();
        assert!(kinds
            .iter()
            .any(|(k, n)| *k == "class" && n.contains("Router")));
        assert!(kinds.iter().any(|(k, n)| *k == "method" && *n == "call"));
        assert!(kinds.iter().any(|(k, n)| *k == "method" && *n == "handle"));
    }

    // --- System prompt tool hierarchy ---

    #[test]
    fn test_system_prompt_context_is_primary() {
        assert!(
            CODESYNAPSE_INSTRUCTIONS.contains("ALWAYS call codesynapse_context FIRST"),
            "system prompt must designate codesynapse_context as primary tool"
        );
    }

    #[test]
    fn test_system_prompt_resolve_is_step2() {
        assert!(
            CODESYNAPSE_INSTRUCTIONS.contains("codesynapse_resolve"),
            "system prompt must reference resolve as step 2"
        );
    }

    #[test]
    fn test_system_prompt_resolve_trigger() {
        assert!(
            CODESYNAPSE_INSTRUCTIONS.contains("No exact match"),
            "system prompt must tell LLM to use resolve when context returns no exact match"
        );
    }

    #[test]
    fn test_system_prompt_trust_graph_line_kept() {
        assert!(
            CODESYNAPSE_INSTRUCTIONS.contains("Do NOT grep/find to verify graph results"),
            "trust-the-graph line must be kept"
        );
    }

    // --- Sibling hint: context tool detects other methods in same class ---

    #[test]
    fn test_sibling_hint_detects_methods_in_same_class() {
        let source = "public class ProfileDatafetcher {\n\
            public User getUserProfile(DataFetchingEnvironment env) {\n\
                return env.getLocalContext();\n\
            }\n\
            private User queryProfile(DataFetchingEnvironment env) {\n\
                return SecurityUtil.getCurrentUser();\n\
            }\n\
            public void otherMethod() {}\n\
        }\n";
        let items = outline_java(source);
        let class_items: Vec<_> = items.iter().filter(|i| i.kind == "class").collect();
        let method_items: Vec<_> = items.iter().filter(|i| i.kind == "method").collect();
        assert!(!class_items.is_empty(), "expected class item");
        assert!(method_items.len() >= 2, "expected at least 2 method items");

        // Simulate sibling detection for getUserProfile (first method)
        let method = method_items
            .iter()
            .find(|i| i.name == "getUserProfile")
            .unwrap();
        let parent = items
            .iter()
            .filter(|i| i.kind == "class" && i.line <= method.line)
            .max_by_key(|i| i.line)
            .unwrap();
        assert_eq!(parent.name, "ProfileDatafetcher");
        let siblings = items
            .iter()
            .filter(|i| {
                matches!(i.kind.as_str(), "method" | "function")
                    && i.line > parent.line
                    && i.name != method.name
            })
            .count();
        assert!(
            siblings >= 2,
            "expected at least 2 siblings, got {}",
            siblings
        );
    }

    // --- Issue 7: top_k default ---

    #[test]
    fn test_resolve_top_k_default_is_3() {
        let default = McpServer::int_arg(&Map::new(), "top_k", 3);
        assert_eq!(default, 3, "top_k default must be 3");
    }

    // --- Issue 3: Graph cache ---

    #[test]
    fn test_graph_cache_returns_same_arc() {
        use std::sync::Arc;
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Alpha", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let g1 = server.load_module_serve_graph("merged").unwrap();
        let g2 = server.load_module_serve_graph("merged").unwrap();
        assert!(
            Arc::ptr_eq(&g1, &g2),
            "second load must return the cached Arc"
        );
    }

    #[test]
    fn test_graph_cache_cleared_by_build() {
        use std::sync::Arc;
        let tmp = tempfile::tempdir().unwrap();
        write_test_graph(
            tmp.path(),
            "global-graph.json",
            &[("a", "Alpha", "code")],
            &[],
        );
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let g1 = server.load_module_serve_graph("merged").unwrap();
        server.tool_graph_build(&Map::new()).unwrap();
        let g2 = server.load_module_serve_graph("merged").unwrap();
        assert!(
            !Arc::ptr_eq(&g1, &g2),
            "build must clear cache, forcing a fresh load"
        );
    }

    // --- Issue 4: Source cache ---

    #[test]
    fn test_source_cache_populated_after_resolve() {
        let tmp = tempfile::tempdir().unwrap();
        let src = write_source_file(
            tmp.path(),
            "Foo.java",
            "public class Foo {\n  public void run() {}\n}",
        );
        write_graph_with_abs_source(tmp.path(), "global-graph.json", src.to_str().unwrap());
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let mut args = Map::new();
        args.insert("query".into(), serde_json::json!("Foo"));
        server.tool_codesynapse_resolve(&args).unwrap();
        assert!(
            !server.source_cache.lock().unwrap().is_empty(),
            "source cache must be populated after resolve reads a file"
        );
    }

    #[test]
    fn test_source_cache_capped_at_1000() {
        let tmp = tempfile::tempdir().unwrap();
        let real = write_source_file(tmp.path(), "Real.java", "public class Real {}");
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        {
            let mut cache = server.source_cache.lock().unwrap();
            for i in 0..1000usize {
                cache.insert(
                    PathBuf::from(format!("/fake/{}.java", i)),
                    (SystemTime::UNIX_EPOCH, String::new(), Vec::new()),
                );
            }
        }
        assert_eq!(server.source_cache.lock().unwrap().len(), 1000);
        server.get_source_cached(&real);
        let len = server.source_cache.lock().unwrap().len();
        assert!(len < 1000, "cache must be evicted when full (got {})", len);
    }

    #[test]
    fn test_stale_check_returns_false_when_no_modules_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        assert!(!server.is_graph_stale(), "no modules dir → not stale");
    }

    #[test]
    fn test_stale_check_returns_false_when_modules_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path().to_path_buf();
        let modules_dir = global_dir.join("modules").join("mymod");
        std::fs::create_dir_all(&modules_dir).unwrap();
        // Write module graph first, then global graph (global is newer → not stale)
        std::fs::write(modules_dir.join("graph.json"), b"{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(global_dir.join("global-graph.json"), b"{}").unwrap();
        let server = make_server_with_global(setup_store(), global_dir);
        assert!(!server.is_graph_stale());
    }

    #[test]
    fn test_stale_check_returns_true_when_module_newer_than_global() {
        let tmp = tempfile::tempdir().unwrap();
        let global_dir = tmp.path().to_path_buf();
        let modules_dir = global_dir.join("modules").join("mymod");
        std::fs::create_dir_all(&modules_dir).unwrap();
        // Write global graph first, then module graph (module is newer → stale)
        std::fs::write(global_dir.join("global-graph.json"), b"{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(modules_dir.join("graph.json"), b"{}").unwrap();
        let server = make_server_with_global(setup_store(), global_dir);
        assert!(server.is_graph_stale());
    }

    #[test]
    fn test_check_stale_once_caches_result() {
        let tmp = tempfile::tempdir().unwrap();
        let server = make_server_with_global(setup_store(), tmp.path().to_path_buf());
        let first = server.check_stale_once();
        // Manually set stale_check to Some(true) to verify it's not re-computed
        *server.stale_check.lock().unwrap() = Some(true);
        assert!(
            server.check_stale_once(),
            "should return cached true, not re-check"
        );
        let _ = first;
    }

    // --- Fix 1: skip nodes with unreadable source, fallback when all missing ---

    fn make_server_with_module(
        module_name: &str,
        graph_json: serde_json::Value,
    ) -> (McpServer, tempfile::TempDir) {
        let tmpdir = tempfile::TempDir::new().unwrap();
        let graph_path = tmpdir.path().join(format!("{}.json", module_name));
        std::fs::write(&graph_path, graph_json.to_string()).unwrap();
        let manifest = serde_json::json!({
            "version": 1,
            "repos": {
                module_name: {
                    "added_at": "2024-01-01",
                    "source_path": graph_path.to_str().unwrap(),
                    "node_count": 1,
                    "edge_count": 0,
                    "source_hash": ""
                }
            }
        });
        std::fs::write(
            tmpdir.path().join("global-manifest.json"),
            manifest.to_string(),
        )
        .unwrap();
        let global_dir = tmpdir.path().to_path_buf();
        let server = McpServer {
            backend: Mutex::new(StoreBackend::Memory(MemoryGraphStore::new())),
            graph_path: None,
            last_mtime: Mutex::new(None),
            telemetry: Arc::new(codesynapse_core::telemetry::Telemetry::new(
                global_dir.clone(),
            )),
            global_dir,
            embedder: None,
            node_embeddings: HashMap::new(),
            graph_cache: Mutex::new(HashMap::new()),
            source_cache: Mutex::new(HashMap::new()),
            stale_check: Mutex::new(None),
        };
        (server, tmpdir)
    }

    #[test]
    fn test_context_skips_node_with_missing_source() {
        // Two nodes with same label — one readable, one dead. Only the readable one should render.
        let src_dir = tempfile::TempDir::new().unwrap();
        let src_file = src_dir.path().join("real.py");
        std::fs::write(&src_file, "def authenticate():\n    pass\n").unwrap();

        let graph = serde_json::json!({
            "nodes": [
                {
                    "id": "real_node",
                    "label": "authenticate",
                    "source_file": src_file.to_str().unwrap(),
                    "file_type": "code"
                },
                {
                    "id": "dead_node",
                    "label": "authenticate",
                    "source_file": "/nonexistent/dead.py",
                    "file_type": "code"
                }
            ],
            "edges": []
        });

        let (server, _dir) = make_server_with_module("fix1-skip", graph);
        let mut args = Map::new();
        args.insert("query".into(), "authenticate".into());
        args.insert("graph".into(), "fix1-skip".into());
        let result = server.tool_codesynapse_context(&args).unwrap();

        assert!(
            !result.contains("dead.py"),
            "node with missing source should not appear; output: {}",
            &result[..result.len().min(300)]
        );
        assert!(
            result.contains("Exact match found"),
            "readable node should still produce exact match; output: {}",
            &result[..result.len().min(300)]
        );
    }

    #[test]
    fn test_context_fallback_when_all_sources_missing() {
        let graph = serde_json::json!({
            "nodes": [
                {
                    "id": "dead_node",
                    "label": "authenticate",
                    "source_file": "/nonexistent/dead.py",
                    "file_type": "code"
                }
            ],
            "edges": []
        });

        let (server, _dir) = make_server_with_module("fix1-fallback", graph);
        let mut args = Map::new();
        args.insert("query".into(), "authenticate".into());
        args.insert("graph".into(), "fix1-fallback".into());
        let result = server.tool_codesynapse_context(&args).unwrap();

        assert!(
            result.starts_with("[No exact match"),
            "when all exact-match sources are missing, should show semantic fallback; got: {}",
            &result[..result.len().min(300)]
        );
    }
}
