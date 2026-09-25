import assert from "node:assert/strict";
import { test } from "node:test";
import { HttpCache } from "../src/cache.js";
import { CanyonHttpClient } from "../src/http.js";

const productUrl = "https://www.canyon.com/en-de/product";

test("rejects disallowed redirects before requesting their targets", async () => {
  const requested: string[] = [];
  const http = new CanyonHttpClient({
    cache: new HttpCache(":memory:"),
    minIntervalMs: 0,
    fetchImpl: async (input) => {
      requested.push(String(input));
      return new Response(null, {
        status: 302,
        headers: { location: "/on/demandware.store/private" },
      });
    },
  });

  await assert.rejects(http.get(productUrl, 0), { code: "ROBOTS_DISALLOWED" });
  assert.deepEqual(requested, [productUrl]);
});

test("follows allowed redirects and preserves the final URL", async () => {
  const destination = "https://www.canyon.com/en-de/new-product";
  const requested: string[] = [];
  const http = new CanyonHttpClient({
    cache: new HttpCache(":memory:"),
    minIntervalMs: 0,
    fetchImpl: async (input) => {
      requested.push(String(input));
      return String(input) === productUrl
        ? new Response(null, { status: 302, headers: { location: destination } })
        : new Response("product", { status: 200 });
    },
  });

  const result = await http.get(productUrl, 0);
  assert.deepEqual(requested, [productUrl, destination]);
  assert.equal(result.url, destination);
  assert.equal(result.body, "product");
});

test("spaces concurrent requests by the configured minimum interval", async () => {
  const starts: number[] = [];
  const http = new CanyonHttpClient({
    cache: new HttpCache(":memory:"),
    minIntervalMs: 80,
    fetchImpl: async () => {
      starts.push(performance.now());
      return new Response("ok", { status: 200 });
    },
  });

  await Promise.all([
    http.get(productUrl, 0),
    http.get("https://www.canyon.com/en-de/another-product", 0),
  ]);
  assert.equal(starts.length, 2);
  assert.ok(starts[1]! - starts[0]! >= 70, `Requests were ${starts[1]! - starts[0]!} ms apart`);
});

test("applies the minimum interval to retries", async () => {
  const starts: number[] = [];
  const http = new CanyonHttpClient({
    cache: new HttpCache(":memory:"),
    minIntervalMs: 1_200,
    fetchImpl: async () => {
      starts.push(performance.now());
      return new Response(null, { status: starts.length === 1 ? 429 : 200 });
    },
  });

  await http.get(productUrl, 0);
  assert.equal(starts.length, 2);
  assert.ok(starts[1]! - starts[0]! >= 1_150, `Retry was ${starts[1]! - starts[0]!} ms later`);
});

test("does not return stale data after a permanent HTTP error", async () => {
  const cache = new HttpCache(":memory:");
  cache.put({ url: productUrl, body: "old product", fetchedAt: "2020-01-01T00:00:00.000Z" });
  const http = new CanyonHttpClient({
    cache,
    minIntervalMs: 0,
    fetchImpl: async () => new Response(null, { status: 404 }),
  });

  await assert.rejects(http.get(productUrl, 0), { code: "HTTP_ERROR" });
});

test("retains stale fallback for temporary HTTP failures", async () => {
  const cache = new HttpCache(":memory:");
  cache.put({ url: productUrl, body: "old product", fetchedAt: "2020-01-01T00:00:00.000Z" });
  const http = new CanyonHttpClient({
    cache,
    minIntervalMs: 0,
    fetchImpl: async () => new Response(null, { status: 503 }),
  });

  const result = await http.get(productUrl, 0);
  assert.equal(result.body, "old product");
  assert.equal(result.stale, true);
});
