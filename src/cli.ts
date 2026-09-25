#!/usr/bin/env bun
import { writeFile } from "node:fs/promises";
import { Command, InvalidArgumentError, Option } from "commander";
import { CanyonClient } from "./client.js";
import { CanyonError } from "./errors.js";
import { formatComparison, formatProduct, formatSearchTable, toCsv } from "./format.js";
import type { SearchOptions } from "./types.js";

const VERSION = "0.1.0";
const program = new Command();
program
  .name("canyon")
  .description("Query Canyon's public bicycle catalog without using private commerce endpoints")
  .version(VERSION)
  .showSuggestionAfterError();

function addSearchOptions(command: Command): Command {
  return command
    .option("-l, --locale <locale>", "Canyon language-country locale", "en-de")
    .option("-w, --world <world>", "road, gravel, mountain, e-bike, city/touring, or triathlon")
    .option("-s, --size <size>", "frame size, for example M or 2XL")
    .option("--in-stock", "only models with at least one matching in-stock variant")
    .addOption(
      new Option("--sort <order>", "sort order")
        .choices(["availability", "price-asc", "price-desc", "newest", "bestseller"])
        .default("availability"),
    )
    .option("--limit <number>", "maximum result count (1–200)", resultLimit, 25)
    .option("--refresh", "bypass fresh cached responses")
    .option("--include-gear", "include clothing and parts/accessories in results");
}

function positiveInteger(value: string): number {
  if (!/^[1-9]\d*$/.test(value) || !Number.isSafeInteger(Number(value))) {
    throw new InvalidArgumentError(`Expected a positive whole number, received '${value}'.`);
  }
  return Number(value);
}

function resultLimit(value: string): number {
  const limit = positiveInteger(value);
  if (limit > 200) throw new InvalidArgumentError("The maximum result count is 200.");
  return limit;
}

function searchOptions(query: string | undefined, options: Record<string, unknown>): SearchOptions {
  return {
    ...(query ? { query } : {}),
    locale: String(options.locale),
    ...(options.world ? { world: String(options.world) } : {}),
    ...(options.size ? { size: String(options.size) } : {}),
    inStock: Boolean(options.inStock),
    ...(options.sort ? { sort: options.sort as NonNullable<SearchOptions["sort"]> } : {}),
    limit: Number(options.limit),
    refresh: Boolean(options.refresh),
    includeGear: Boolean(options.includeGear),
  };
}

addSearchOptions(program.command("search [query]").description("Search and filter bicycle models"))
  .option("--json", "print machine-readable JSON")
  .action(async (query: string | undefined, options: Record<string, unknown>) => {
    const result = await new CanyonClient().search(searchOptions(query, options));
    if (options.json) console.log(JSON.stringify(result, null, 2));
    else {
      console.log(formatSearchTable(result.products));
      console.error(`\nShowing ${result.products.length} of ${result.total}; fetched ${result.fetchedAt}${result.stale ? " (stale cache)" : ""}`);
    }
  });

program
  .command("product <identifier>")
  .description("Get a product by model ID, SKU, or full Canyon URL")
  .option("-l, --locale <locale>", "Canyon language-country locale", "en-de")
  .option("--refresh", "bypass fresh cached responses")
  .option("--json", "print machine-readable JSON")
  .action(async (identifier: string, options: Record<string, unknown>) => {
    const product = await new CanyonClient().getProduct(identifier, {
      locale: String(options.locale),
      refresh: Boolean(options.refresh),
    });
    console.log(options.json ? JSON.stringify(product, null, 2) : formatProduct(product));
  });

program
  .command("compare <identifiers...>")
  .description("Compare 2–5 products by model ID, SKU, or URL")
  .option("-l, --locale <locale>", "Canyon language-country locale", "en-de")
  .option("--refresh", "bypass fresh cached responses")
  .option("--json", "print machine-readable JSON")
  .action(async (identifiers: string[], options: Record<string, unknown>) => {
    const products = await new CanyonClient().compare(identifiers, {
      locale: String(options.locale),
      refresh: Boolean(options.refresh),
    });
    console.log(options.json ? JSON.stringify(products, null, 2) : formatComparison(products));
  });

addSearchOptions(program.command("export [query]").description("Export search results as JSON or CSV"))
  .addOption(new Option("-f, --format <format>", "output format").choices(["json", "csv"]).default("json"))
  .option("-o, --output <path>", "write to a file instead of stdout")
  .action(async (query: string | undefined, options: Record<string, unknown>) => {
    const result = await new CanyonClient().search(searchOptions(query, options));
    const output = options.format === "csv" ? toCsv(result.products) : JSON.stringify(result, null, 2) + "\n";
    if (options.output) {
      await writeFile(String(options.output), output, "utf8");
      console.error(`Wrote ${result.products.length} products to ${String(options.output)}`);
    } else process.stdout.write(output);
  });

program
  .command("locales")
  .description("List locales published in Canyon's sitemap index")
  .option("--refresh", "bypass fresh cached responses")
  .option("--json", "print machine-readable JSON")
  .action(async (options: Record<string, unknown>) => {
    const locales = await new CanyonClient().listLocales(Boolean(options.refresh));
    console.log(options.json ? JSON.stringify(locales, null, 2) : locales.map((entry) => entry.locale).join("\n"));
  });

program
  .command("watch <identifier>")
  .description("Poll a model and report variant stock changes; stop with Ctrl-C")
  .option("-l, --locale <locale>", "Canyon language-country locale", "en-de")
  .option("-s, --size <size>", "only report this frame size")
  .option("--interval <seconds>", "poll interval; minimum 60", positiveInteger, 300)
  .option("--once", "check once and exit")
  .action(async (identifier: string, options: Record<string, unknown>) => {
    const interval = Math.max(Number(options.interval), 60);
    const client = new CanyonClient();
    let resolvedIdentifier = identifier;
    let previous = "";
    do {
      const product = await client.getProduct(resolvedIdentifier, { locale: String(options.locale), refresh: true });
      resolvedIdentifier = product.url;
      const size = options.size ? String(options.size).toUpperCase() : undefined;
      const variants = product.variants.filter((variant) => !size || variant.size?.toUpperCase() === size);
      const snapshot = JSON.stringify(variants.map(({ sku, color, size: variantSize, availability, price }) => ({
        sku, color, size: variantSize, availability, price,
      })));
      if (snapshot !== previous) {
        console.log(`[${new Date().toISOString()}] ${product.name}`);
        console.log(formatProduct({ ...product, variants }));
        previous = snapshot;
      }
      if (!options.once) await new Promise((resolve) => setTimeout(resolve, interval * 1000));
    } while (!options.once);
  });

program
  .command("cache")
  .description("Show HTTP cache statistics")
  .action(() => console.log(JSON.stringify(new CanyonClient().cacheStats(), null, 2)));

try {
  await program.parseAsync(process.argv);
} catch (error) {
  if (error instanceof CanyonError) {
    console.error(`canyon: ${error.message} [${error.code}]`);
    process.exitCode = 1;
  } else {
    throw error;
  }
}
