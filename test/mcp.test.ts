import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";

test("stdio MCP server completes handshake and lists tools", async () => {
  const projectRoot = fileURLToPath(new URL("..", import.meta.url));
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: ["src/mcp.ts"],
    cwd: projectRoot,
    stderr: "pipe",
  });
  const client = new Client({ name: "canyon-mcp-test", version: "0.1.0" });
  try {
    await client.connect(transport);
    const result = await client.listTools();
    assert.deepEqual(
      result.tools.map((tool) => tool.name).sort(),
      ["compare_bikes", "get_bike", "list_locales", "search_bikes"],
    );
  } finally {
    await client.close();
  }
});
