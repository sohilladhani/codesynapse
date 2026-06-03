/**
 * Integration tests for codesynapse-pi MCP subprocess communication.
 * These tests require the codesynapse binary to be built and available.
 *
 * Run with: npx tsx --test evals/mcp-integration.test.ts
 */

import { describe, it, before, after } from "node:test";
import assert from "node:assert";

// Re-import the MCP client from the extension source
import { CodesynapseMCPClient } from "../src/extension.ts";

describe("MCP subprocess integration", { timeout: 30_000 }, () => {
  let client: CodesynapseMCPClient;

  before(() => {
    client = new CodesynapseMCPClient("codesynapse", ["mcp"]);
  });

  after(() => {
    client.close();
  });

  it("connects to MCP server and lists tools", async () => {
    await client.ensureRunning();
    const tools = await client.listTools();
    assert(Array.isArray(tools));
    assert(tools.length >= 32);
    assert(tools.some((t: any) => t.name === "codesynapse_context"));
  });

  it("calls codesynapse_stats tool", async () => {
    const result = (await client.callTool("codesynapse_stats", {})) as any;
    assert(result?.content?.[0]?.text);
    assert(typeof result.content[0].text === "string");
  });

  it("auto-reconnects on crash", async () => {
    // Kill the current process
    client.close();

    // Should respawn on next call
    await client.ensureRunning();
    const tools = await client.listTools();
    assert(Array.isArray(tools));
    assert(tools.length >= 32);
  });

  it("handles unknown tool error gracefully", async () => {
    try {
      await client.callTool("nonexistent_tool", {});
      assert.fail("Should have thrown");
    } catch (err: unknown) {
      assert(err instanceof Error);
      assert(err.message.includes("MCP error") || err.message.includes("not found"));
    }
  });
});
