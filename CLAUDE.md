# Codesynapse — Code Intelligence

Tool selection and usage instructions are delivered via MCP `initialize` — no duplication needed here.

## Displaying codesynapse_stats

When the user calls `codesynapse_stats`, ALWAYS display the tool result verbatim in a code fence:

```
[paste the entire codesynapse_stats output here]
```

The output contains Unicode box-drawing characters (╔═╗║╠╣╚╝├┤│) and bar charts (█░) that must be preserved exactly as returned. Do NOT paraphrase or summarize the dashboard — users need to see the visual formatting.

<!-- codesynapse:start -->
## Codesynapse — Code Intelligence

Use these CLI commands to answer architecture questions (works in subagents without MCP):
- `codesynapse resolve "how does X work"` — hybrid search + source body
- `codesynapse query "concept name"` — find relevant symbols

Repository indexed as module `tokio`. Re-index: `codesynapse module add --force tokio <path>`.
<!-- codesynapse:end -->
