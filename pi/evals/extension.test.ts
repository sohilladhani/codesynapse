/**
 * Unit tests for codesynapse-pi extension behavior.
 * These tests verify extension logic without a real codesynapse MCP server.
 * Run with: node --test evals/*.test.ts
 */

import { describe, it } from "node:test";
import assert from "node:assert";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

// ---------------------------------------------------------------------------
// Mock helpers
// ---------------------------------------------------------------------------

interface MockAPI {
  registeredTools: { name: string }[];
  registeredCommands: string[];
  eventHandlers: Map<string, Function[]>;
  cliAvailable: boolean;
}

function createMockAPI(options?: { cliAvailable?: boolean }): MockAPI {
  return {
    registeredTools: [],
    registeredCommands: [],
    eventHandlers: new Map(),
    cliAvailable: options?.cliAvailable ?? true,
  };
}

function createMockPi(api: MockAPI): ExtensionAPI {
  return {
    registerTool(tool: any) {
      api.registeredTools.push(tool);
    },
    registerCommand(name: string) {
      api.registeredCommands.push(name);
    },
    on(event: string, handler: Function) {
      if (!api.eventHandlers.has(event)) {
        api.eventHandlers.set(event, []);
      }
      api.eventHandlers.get(event)!.push(handler);
    },
    sendMessage() {},
    sendUserMessage() {},
    appendEntry() {},
    setSessionName() {},
    getSessionName() { return undefined; },
    setLabel() {},
    getCommands() { return []; },
    getActiveTools() { return []; },
    getAllTools() { return []; },
    setActiveTools() {},
    getFlag() { return undefined; },
    setModel() { return true; },
    getThinkingLevel() { return "off" as const; },
    setThinkingLevel() {},
    events: { on() {}, emit() {} },
    registerProvider() {},
    unregisterProvider() {},
    registerShortcut() {},
    registerFlag() {},
    exec() { return Promise.resolve({ stdout: "", stderr: "", code: 0, killed: false, signal: undefined }); },
    registerMessageRenderer() {},
  } as unknown as ExtensionAPI;
}

async function loadExtension(api: MockAPI): Promise<void> {
  const pi = createMockPi(api);
  const mod = await import("../src/extension.ts");
  mod.createExtension(pi, { isCliAvailable: () => Promise.resolve(api.cliAvailable) });
}

async function fireBeforeAgentStart(api: MockAPI, systemPrompt: string) {
  const handlers = api.eventHandlers.get("before_agent_start") ?? [];
  let currentPrompt = systemPrompt;
  for (const handler of handlers) {
    const result = await handler(
      { systemPrompt: currentPrompt, systemPromptOptions: {} } as any,
      {
        ui: {} as any,
        cwd: process.cwd(),
        mode: "test" as any,
        hasUI: false,
        signal: undefined,
        isIdle: () => true,
        abort: () => {},
        hasPendingMessages: () => false,
        shutdown: () => {},
        getContextUsage: () => undefined,
        compact: () => {},
        getSystemPrompt: () => currentPrompt,
        sessionManager: {} as any,
        modelRegistry: {} as any,
        model: undefined,
        isProjectTrusted: () => false,
      } as any,
    );
    if (result?.systemPrompt) {
      currentPrompt = result.systemPrompt;
    }
  }
  return { systemPrompt: currentPrompt };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("codesynapse-pi extension", () => {
  it("registers 12 tools", async () => {
    const api = createMockAPI();
    await loadExtension(api);

    assert.equal(api.registeredTools.length, 12);
    assert(api.registeredTools.some(t => t.name === "codesynapse_context"));
    assert(api.registeredTools.some(t => t.name === "codesynapse_resolve"));
    assert(api.registeredTools.some(t => t.name === "codesynapse_build"));
  });

  it("registers /codesynapse command", async () => {
    const api = createMockAPI();
    await loadExtension(api);

    assert(api.registeredCommands.includes("codesynapse"));
  });

  it("injects codesynapse guidance when CLI is available", async () => {
    const api = createMockAPI({ cliAvailable: true });
    await loadExtension(api);

    const result = await fireBeforeAgentStart(api, "Default system prompt");
    assert(result.systemPrompt.includes("Codesynapse Knowledge Graph"));
    assert(result.systemPrompt.includes("codesynapse_context"));
  });

  it("skips injection when CLI is not available", async () => {
    const api = createMockAPI({ cliAvailable: false });
    await loadExtension(api);

    const result = await fireBeforeAgentStart(api, "Default system prompt");
    assert.equal(result.systemPrompt, "Default system prompt");
  });

  it("registers session_start and session_shutdown handlers", async () => {
    const api = createMockAPI();
    await loadExtension(api);

    assert(api.eventHandlers.has("session_start"));
    assert(api.eventHandlers.has("session_shutdown"));
  });
});
