#!/usr/bin/env bun
import { McpServer } from "@modelcontextprotocol/server";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import * as z from "zod/v4";
import { CanyonClient } from "./client.js";
import { CanyonError } from "./errors.js";
import { formatComparison, formatProduct, formatSearchTable } from "./format.js";

const localeSchema = z.string().regex(/^[a-z]{2}[-_][a-z]{2}$/i).default("en-de");

function success(text: string, data: unknown) {
  return {
    content: [{ type: "text" as const, text }],
    structuredContent: { data },
  };
}

function failure(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  const code = error instanceof CanyonError ? error.code : "UNEXPECTED_ERROR";
  return {
    isError: true,
    content: [{ type: "text" as const, text: `${message} [${code}]` }],
    structuredContent: { error: { code, message } },
  };
}

export function createServer(): McpServer {
  const server = new McpServer(
    { name: "canyon-catalog", version: "0.1.0" },
    {
      instructions:
        "Read-only access to Canyon's public bicycle catalog. Search before fetching details when the model ID is unknown. Prices and availability are locale-specific and timestamped. Never claim cached availability is current when stale is true.",
    },
  );
  const client = new CanyonClient();

  server.registerTool(
    "search_bikes",
    {
      description: "Search Canyon bicycle models with optional locale, world, size, stock, and sorting filters.",
      inputSchema: z.object({
        query: z.string().optional(),
        locale: localeSchema,
        world: z.string().optional().describe("road, gravel, mountain, e-bike, city/touring, or triathlon"),
        size: z.string().optional(),
        inStock: z.boolean().default(false),
        sort: z.enum(["availability", "price-asc", "price-desc", "newest", "bestseller"]).default("availability"),
        limit: z.number().int().min(1).max(200).default(25),
        refresh: z.boolean().default(false),
        includeGear: z.boolean().default(false),
      }),
    },
    async (input) => {
      try {
        const result = await client.search({
          ...(input.query ? { query: input.query } : {}),
          locale: input.locale,
          ...(input.world ? { world: input.world } : {}),
          ...(input.size ? { size: input.size } : {}),
          inStock: input.inStock,
          sort: input.sort,
          limit: input.limit,
          refresh: input.refresh,
          includeGear: input.includeGear,
        });
        return success(`${formatSearchTable(result.products)}\n\nFetched: ${result.fetchedAt}`, result);
      } catch (error) {
        return failure(error);
      }
    },
  );

  server.registerTool(
    "get_bike",
    {
      description: "Get detailed variants, stock, geometry, and components by Canyon model ID, SKU, or product URL.",
      inputSchema: z.object({
        identifier: z.string().min(1),
        locale: localeSchema,
        refresh: z.boolean().default(false),
      }),
    },
    async ({ identifier, locale, refresh }) => {
      try {
        const product = await client.getProduct(identifier, { locale, refresh });
        return success(formatProduct(product), product);
      } catch (error) {
        return failure(error);
      }
    },
  );

  server.registerTool(
    "compare_bikes",
    {
      description: "Compare 2–5 Canyon models, including price and important components.",
      inputSchema: z.object({
        identifiers: z.array(z.string().min(1)).min(2).max(5),
        locale: localeSchema,
        refresh: z.boolean().default(false),
      }),
    },
    async ({ identifiers, locale, refresh }) => {
      try {
        const products = await client.compare(identifiers, { locale, refresh });
        return success(formatComparison(products), products);
      } catch (error) {
        return failure(error);
      }
    },
  );

  server.registerTool(
    "list_locales",
    {
      description: "List language-country locales present in Canyon's public sitemap index.",
      inputSchema: z.object({ refresh: z.boolean().default(false) }),
    },
    async ({ refresh }) => {
      try {
        const locales = await client.listLocales(refresh);
        return success(locales.map((entry) => entry.locale).join("\n"), locales);
      } catch (error) {
        return failure(error);
      }
    },
  );

  return server;
}

void serveStdio(createServer);
console.error("canyon-mcp 0.1.0 running on stdio");
