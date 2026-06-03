/**
 * codesynapse-pi extension
 *
 * Brings codesynapse code intelligence into pi via MCP subprocess.
 *
 * Architecture:
 *   - Spawns `codesynapse mcp` as a long-lived child process
 *   - Communicates via JSON-RPC 2.0 over stdin/stdout
 *   - Registers 12 curated pi tools that delegate to the MCP server
 *   - Injects system prompt guidance for the LLM to use codesynapse tools first
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { spawn, type ChildProcess } from "node:child_process";
import { createInterface, type Interface } from "node:readline";
import { resolve } from "node:path";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const REQUEST_TIMEOUT = 30_000;
const SIGTERM_WAIT_MS = 2_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: number | string;
  method: string;
  params?: Record<string, unknown>;
}

interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number | string | null;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

// ---------------------------------------------------------------------------
// MCP Client
// ---------------------------------------------------------------------------

export class CodesynapseMCPClient {
  private proc: ChildProcess | null = null;
  private rl: Interface | null = null;
  private pending = new Map<number | string, PendingRequest>();
  private nextId = 1;
  private running = false;

  constructor(
    private command: string = "codesynapse",
    private args: string[] = ["mcp"],
  ) {}

  get isRunning(): boolean {
    return this.running;
  }

  async ensureRunning(path?: string): Promise<void> {
    if (this.running && this.proc && !this.proc.killed) return;

    if (this.proc && !this.proc.killed) {
      this.killProcess();
    }

    const cmd = path ?? this.command;

    return new Promise<void>((resolvePromise, rejectPromise) => {
      try {
        const child = spawn(cmd, this.args, {
          stdio: ["pipe", "pipe", "pipe"],
          shell: false,
        });

        this.proc = child;
        this.running = true;

        const rl = createInterface({ input: child.stdout!, crlfDelay: Infinity });
        this.rl = rl;

        rl.on("line", (line: string) => {
          this.handleLine(line);
        });

        child.on("error", (err: Error) => {
          if (this.proc !== child) return;
          this.running = false;
          this.rejectAllPending(err);
        });

        child.on("exit", (code: number | null) => {
          // Only handle if this is still the current process
          if (this.proc !== child) return;
          this.running = false;
          const err = new Error(`MCP process exited with code ${code}`);
          this.rejectAllPending(err);
        });

        child.stderr?.on("data", (data: Buffer) => {
          // Log stderr for debugging but don't treat as error
          console.error(`[codesynapse-mcp stderr] ${data.toString().trim()}`);
        });

        // Send initialize request
        const initId = this.nextId++;
        const initRequest: JsonRpcRequest = {
          jsonrpc: "2.0",
          id: initId,
          method: "initialize",
          params: {
            protocolVersion: "2024-11-05",
            capabilities: {},
            clientInfo: { name: "codesynapse-pi", version: "0.1.0" },
          },
        };

        child.stdin!.write(JSON.stringify(initRequest) + "\n");

        const timer = setTimeout(() => {
          this.pending.delete(initId);
          rejectPromise(new Error("MCP initialize timeout"));
        }, REQUEST_TIMEOUT);

        this.pending.set(initId, {
          resolve: () => {
            clearTimeout(timer);
            resolvePromise();
          },
          reject: (err: Error) => {
            clearTimeout(timer);
            rejectPromise(err);
          },
          timer,
        });
      } catch (err) {
        rejectPromise(err as Error);
      }
    });
  }

  close(): void {
    this.killProcess();
    for (const [, p] of this.pending) clearTimeout(p.timer);
    this.pending.clear();
  }

  async callTool(name: string, args: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
    return this.request("tools/call", { name, arguments: args }, signal);
  }

  async listTools(signal?: AbortSignal): Promise<unknown[]> {
    const result = await this.request("tools/list", {}, signal);
    return (result as { tools?: unknown[] })?.tools ?? [];
  }

  private async request(method: string, params: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
    await this.ensureRunning();

    const id = this.nextId++;
    const request: JsonRpcRequest = {
      jsonrpc: "2.0",
      id,
      method,
      params,
    };

    return new Promise<unknown>((resolvePromise, rejectPromise) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        rejectPromise(new Error(`MCP request timed out: ${method}`));
      }, REQUEST_TIMEOUT);

      // Handle abort signal
      const onAbort = () => {
        clearTimeout(timer);
        this.pending.delete(id);
        rejectPromise(new Error(`MCP request aborted: ${method}`));
      };

      if (signal?.aborted) {
        onAbort();
        return;
      }

      signal?.addEventListener("abort", onAbort, { once: true });

      this.pending.set(id, {
        resolve: (val: unknown) => {
          clearTimeout(timer);
          signal?.removeEventListener("abort", onAbort);
          resolvePromise(val);
        },
        reject: (err: Error) => {
          clearTimeout(timer);
          signal?.removeEventListener("abort", onAbort);
          rejectPromise(err);
        },
        timer,
      });

      this.proc?.stdin!.write(JSON.stringify(request) + "\n");
    });
  }

  private handleLine(line: string): void {
    let response: JsonRpcResponse;
    try {
      response = JSON.parse(line);
    } catch {
      return; // skip malformed lines
    }

    if (response.id == null) return; // notification, ignore

    const pending = this.pending.get(response.id);
    if (!pending) return;

    this.pending.delete(response.id);
    clearTimeout(pending.timer);

    if (response.error) {
      pending.reject(new Error(`MCP error ${response.error.code}: ${response.error.message}`));
    } else {
      pending.resolve(response.result);
    }
  }

  private killProcess(): void {
    if (!this.proc || this.proc.killed) return;

    try {
      this.proc.kill("SIGTERM");
    } catch {
      // ignore
    }

    // Force kill after timeout
    setTimeout(() => {
      try {
        if (this.proc && !this.proc.killed) {
          this.proc.kill("SIGKILL");
        }
      } catch {
        // ignore
      }
    }, SIGTERM_WAIT_MS).unref();

    this.running = false;
    this.rl?.close();
    this.rl = null;
    this.proc = null;
  }

  private rejectAllPending(err: Error): void {
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pending.clear();
  }
}

// ---------------------------------------------------------------------------
// Helper: check if CLI is available
// ---------------------------------------------------------------------------

async function isCliAvailable(command: string): Promise<boolean> {
  try {
    const { execSync } = await import("node:child_process");
    execSync(`which ${command}`, { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

interface ToolDefinition {
  name: string;
  label: string;
  description: string;
  promptSnippet?: string;
  promptGuidelines?: string[];
  parameters: ReturnType<typeof Type.Object>;
}

const TOOL_DEFINITIONS: ToolDefinition[] = [
  {
    name: "codesynapse_context",
    label: "Codesynapse Context",
    description:
      "PRIMARY tool — call this FIRST for architecture/mechanism questions. Accepts natural language or symbol names, finds entry points, expands one hop via call graph edges, and returns full source bodies with line numbers. Falls back to BM25+dense semantic search when no exact match exists.",
    promptSnippet: "Answer architecture questions via codesynapse knowledge graph",
    promptGuidelines: [
      "Use codesynapse_context as the PRIMARY tool for 'how does X work' questions",
      "Accepts natural language or symbol names",
      "Returns full source bodies — answer directly from output",
    ],
    parameters: Type.Object({
      query: Type.String({ description: "Natural-language question or symbol name" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
      max_chars: Type.Optional(Type.Number({ default: 16000 })),
    }),
  },
  {
    name: "codesynapse_resolve",
    label: "Codesynapse Resolve",
    description:
      "BM25+dense hybrid search fallback. Use when codesynapse_context fails to find what you need. Returns ranked symbol matches with source snippets.",
    promptGuidelines: [
      "Use codesynapse_resolve when codesynapse_context returns no exact match",
    ],
    parameters: Type.Object({
      query: Type.String({ description: "Search query (natural language or symbol)" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
      limit: Type.Optional(Type.Number({ default: 10 })),
    }),
  },
  {
    name: "codesynapse_stats",
    label: "Codesynapse Stats",
    description: "Session dashboard showing graph health, node/edge counts, token savings from using the graph instead of grep/search.",
    promptGuidelines: [
      "Use codesynapse_stats to check graph health and session token savings",
    ],
    parameters: Type.Object({}),
  },
  {
    name: "codesynapse_blast_radius",
    label: "Codesynapse Blast Radius",
    description: "Find everything affected by changing a class or function. Returns all callers, callees, and transitive dependents.",
    promptGuidelines: [
      "Use codesynapse_blast_radius before modifying a class to understand impact",
    ],
    parameters: Type.Object({
      symbol: Type.String({ description: "Class or function name to analyze" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
      depth: Type.Optional(Type.Number({ default: 2 })),
    }),
  },
  {
    name: "codesynapse_hierarchy",
    label: "Codesynapse Hierarchy",
    description: "Class inheritance tree for a given class. Shows parent classes, subclasses, and implemented traits/interfaces.",
    promptGuidelines: [
      "Use codesynapse_hierarchy to understand class inheritance before editing a class hierarchy",
    ],
    parameters: Type.Object({
      class_name: Type.String({ description: "Class name to get hierarchy for" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
    }),
  },
  {
    name: "codesynapse_list_graphs",
    label: "Codesynapse List Graphs",
    description: "List all available module graphs. Returns module names that can be used with other tools.",
    parameters: Type.Object({}),
  },
  {
    name: "codesynapse_module_summary",
    label: "Codesynapse Module Summary",
    description: "Module overview showing node count, edge count, top-level symbols, and god-nodes (highly connected symbols).",
    parameters: Type.Object({
      module_name: Type.String({ description: "Module name from codesynapse_list_graphs" }),
    }),
  },
  {
    name: "codesynapse_outline",
    label: "Codesynapse Outline",
    description: "Compact class/module structure showing methods, fields, and line numbers. Faster than reading the full file.",
    parameters: Type.Object({
      symbol: Type.String({ description: "Class or module name" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
    }),
  },
  {
    name: "codesynapse_read_method",
    label: "Codesynapse Read Method",
    description: "Get the exact body of a method via brace tracking. Returns complete source with line numbers.",
    promptGuidelines: [
      "Use codesynapse_read_method when you need full method bodies rather than snippets",
    ],
    parameters: Type.Object({
      class_name: Type.String({ description: "Class or type name" }),
      method: Type.String({ description: "Method/function name" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
    }),
  },
  {
    name: "codesynapse_find_callers",
    label: "Codesynapse Find Callers",
    description: "Find all functions that call a specific class method. Shows file paths and line numbers.",
    promptGuidelines: [
      "Use codesynapse_find_callers to trace who depends on a specific method",
    ],
    parameters: Type.Object({
      class_name: Type.String({ description: "Class or type name" }),
      method: Type.String({ description: "Method/function name" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
    }),
  },
  {
    name: "codesynapse_find_usages",
    label: "Codesynapse Find Usages",
    description: "Find all files that reference a class, type, or symbol. Shows file paths and line numbers.",
    parameters: Type.Object({
      symbol: Type.String({ description: "Symbol name (class, type, etc.)" }),
      graph: Type.Optional(Type.String({ default: "merged" })),
    }),
  },
  {
    name: "codesynapse_build",
    label: "Codesynapse Build",
    description: "Reload the code graph after module changes. Must be called after adding new modules or when the graph is stale.",
    promptGuidelines: [
      "Use codesynapse_build to reload the graph after editing module config or adding source files",
    ],
    parameters: Type.Object({
      path: Type.Optional(Type.String({ description: "Path to project root (defaults to current directory)" })),
    }),
  },
];

// ---------------------------------------------------------------------------
// Extension entry point
// ---------------------------------------------------------------------------

interface ExtensionOptions {
  /** Override CLI availability check (for testing) */
  isCliAvailable?: () => Promise<boolean>;
  /** Path to codesynapse binary (default: auto-detect) */
  mcpCommand?: string;
}

/**
 * Auto-detect the codesynapse binary.
 * Prefers local dev build (./target/debug/codesynapse), then PATH.
 */
export async function detectCodesynapseBinary(cwd: string): Promise<string | null> {
  // Check for local dev build first
  const { access } = await import("node:fs/promises");
  const { join } = await import("node:path");
  const devBuild = join(cwd, "target", "debug", "codesynapse");
  try {
    await access(devBuild);
    return devBuild;
  } catch {
    // Not found, fall through
  }
  // Check PATH
  try {
    const { execSync } = await import("node:child_process");
    execSync("which codesynapse", { stdio: "ignore" });
    return "codesynapse";
  } catch {
    return null;
  }
}

export function createExtension(pi: ExtensionAPI, opts?: ExtensionOptions) {
  const cmd = opts?.mcpCommand ?? "codesynapse";
  const client = new CodesynapseMCPClient(cmd);
  let cliChecked = false;
  let cliAvailable = false;

  const checkCli = opts?.isCliAvailable ?? (async () => {
    const binary = await detectCodesynapseBinary(process.cwd());
    return binary !== null;
  });

  // -----------------------------------------------------------------------
  // Helper: handle tool calls by delegating to MCP server
  // -----------------------------------------------------------------------

  async function handleToolCall(
    toolName: string,
    params: Record<string, unknown>,
    signal: AbortSignal | undefined,
    _ctx: unknown,
  ): Promise<{ content: { type: "text"; text: string }[]; details: Record<string, unknown> }> {
    try {
      await client.ensureRunning();

      const result = (await client.callTool(toolName, params, signal)) as {
        content?: { type?: string; text?: string }[];
        isError?: boolean;
      };

      const content = result?.content ?? [{ type: "text" as const, text: `Tool ${toolName} returned no content` }] as { type: "text"; text: string }[];

      return {
        content: content.map((c: { type?: string; text?: string }) => ({
          type: "text" as const,
          text: c.text ?? "",
        })),
        details: {},
      };
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);

      // Detect CLI not found errors
      if (message.includes("spawn") || message.includes("ENOENT") || message.includes("not found")) {
        return {
          content: [{ type: "text" as const, text: `Codesynapse not found. Install with \`cargo install codesynapse\` or build from source.\n\nError: ${message}` }],
          details: {},
        };
      }

      return {
        content: [{ type: "text" as const, text: `Error calling ${toolName}: ${message}` }],
        details: {},
      };
    }
  }

  // -----------------------------------------------------------------------
  // Register 12 tools
  // -----------------------------------------------------------------------

  for (const def of TOOL_DEFINITIONS) {
    pi.registerTool({
      name: def.name,
      label: def.label,
      description: def.description,
      promptSnippet: def.promptSnippet,
      promptGuidelines: def.promptGuidelines,
      parameters: def.parameters,

      async execute(
        _toolCallId: string,
        params: Record<string, unknown>,
        signal: AbortSignal | undefined,
        _onUpdate: unknown,
        _ctx: unknown,
      ) {
        return handleToolCall(def.name, params, signal, undefined);
      },
    });
  }

  // -----------------------------------------------------------------------
  // Register /codesynapse command
  // -----------------------------------------------------------------------

  pi.registerCommand("codesynapse", {
    description: "Check codesynapse MCP connectivity and graph status",
    handler: async (_args: string, ctx: any) => {
      try {
        const result = await handleToolCall("codesynapse_stats", {}, undefined, undefined);
        ctx.ui.notify(result.content[0].text, "info");
      } catch {
        ctx.ui.notify("Codesynapse not available. Is it installed?", "error");
      }
    },
  });

  // -----------------------------------------------------------------------
  // System prompt injection (before_agent_start)
  // -----------------------------------------------------------------------

  pi.on("before_agent_start" as any, async (event: any, _ctx: any) => {
    // Check CLI availability once per session
    if (!cliChecked) {
      cliChecked = true;
      cliAvailable = await checkCli();
    }

    if (!cliAvailable) return undefined;

    return {
      systemPrompt:
        event.systemPrompt +
        `

## Codesynapse Knowledge Graph

Codesynapse is available (codesynapse MCP tools are registered).
- codesynapse_context(query) is the PRIMARY tool — call it FIRST for architecture/mechanism questions.
  It searches ALL indexed modules automatically. No need to specify a graph.
- If results seem incomplete, try a more specific query or use codesynapse_resolve().
`,
    };
  });

  // -----------------------------------------------------------------------
  // Lifecycle hooks
  // -----------------------------------------------------------------------

  pi.on("session_start" as any, async () => {
    // Pre-warm: spawn MCP process in background while user types first prompt
    // This absorbs the ~7.5s graph load time so the first tool call is instant
    client.ensureRunning().catch(() => {});
  });

  pi.on("session_shutdown" as any, async () => {
    client.close();
  });
}

/** Default export for pi extension loader */
/** Auto-detect binary and create extension */
export default async function (pi: ExtensionAPI) {
  const binary = await detectCodesynapseBinary(process.cwd());
  createExtension(pi, { mcpCommand: binary ?? undefined });
}
