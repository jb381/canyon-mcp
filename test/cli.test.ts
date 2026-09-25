import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const projectRoot = fileURLToPath(new URL("..", import.meta.url));

test("rejects invalid numeric options before making a request", () => {
  for (const args of [
    ["search", "grail", "--limit", "nope"],
    ["search", "grail", "--limit", "201"],
    ["watch", "4509", "--interval", "0"],
  ]) {
    const result = Bun.spawnSync([process.execPath, "src/cli.ts", ...args], {
      cwd: projectRoot,
      stdout: "pipe",
      stderr: "pipe",
    });
    assert.notEqual(result.exitCode, 0, `Expected ${args.join(" ")} to fail`);
    assert.match(result.stderr.toString(), /error: option/i);
  }
});
