import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import {
  parseAllBikesUrl,
  parseProductPage,
  parseSearchFilterLinks,
  parseSearchPage,
  parseSitemapIndex,
} from "../src/parser.js";

const fixture = (name: string) => readFile(new URL(`./fixtures/${name}`, import.meta.url), "utf8");

test("parses ProductGroup JSON-LD and variants", async () => {
  const product = parseProductPage(
    await fixture("product.html"),
    "https://www.canyon.com/en-de/gravel-bikes/example/4513.html",
    "2026-09-22T00:00:00.000Z",
  );
  assert.equal(product.modelId, "4513");
  assert.equal(product.attributes.Weight, "8.39 kg");
  assert.equal(product.variants.length, 1);
  assert.deepEqual(product.variants[0]?.price, { amount: 3999, currency: "EUR" });
  assert.equal(product.variants[0]?.availability, "in_stock");
  assert.equal(product.variants[0]?.attributes.Stack, "575 mm");
});

test("parses server-rendered search tiles", async () => {
  const result = parseSearchPage(
    await fixture("search.html"),
    "https://www.canyon.com/en-de/lp/bicycle/?q=grail",
    true,
  );
  assert.equal(result.total, 1);
  assert.equal(result.products[0]?.modelId, "4513");
  assert.equal(result.products[0]?.sku, "50000001");
  assert.equal(result.products[0]?.availability, "in_stock");
  assert.deepEqual(result.products[0]?.price, { amount: 3999, currency: "EUR" });
});

test("does not turn an empty analytics price into zero", async () => {
  const result = parseSearchPage(
    await fixture("search.html"),
    "https://www.canyon.com/en-de/lp/bicycle/",
  );
  assert.deepEqual(result.products[0]?.price, { amount: 3999, currency: "EUR" });
});

test("parses localized filter values and all-bike links", () => {
  const html = `
    <a data-eventlabel="All Bikes" href="/de-de/lp/fahrrad/">Alle Fahrräder</a>
    <button class="js-productFilterButton" data-url="/de-de/lp/fahrrad/?prefn1=isInStock&amp;prefv1=Auf%20Lager" data-gtm-click='[{"event":"product_filter","filter_name":"isInStock","filter_value":"In-stock"}]'></button>
    <button class="js-productFilterButton" data-url="/de-de/lp/fahrrad/?prefn1=pc_welt&amp;prefv1=Rennrad" data-gtm-click='[{"event":"product_filter","filter_name":"pc_welt","filter_value":"Road"}]'></button>`;
  assert.equal(parseAllBikesUrl(html, "https://www.canyon.com/de-de/"), "https://www.canyon.com/de-de/lp/fahrrad/");
  assert.deepEqual(parseSearchFilterLinks(html, "https://www.canyon.com/de-de/"), [
    { name: "isInStock", canonicalValue: "In-stock", actualValue: "Auf Lager" },
    { name: "pc_welt", canonicalValue: "Road", actualValue: "Rennrad" },
  ]);
});

test("parses locale sitemap index", () => {
  const xml = `<?xml version="1.0"?><sitemapindex><sitemap><loc>https://www.canyon.com/sitemap-en_DE.xml</loc></sitemap><sitemap><loc>https://www.canyon.com/sitemap-de_DE.xml</loc></sitemap></sitemapindex>`;
  assert.deepEqual(parseSitemapIndex(xml), [
    { locale: "en-de", url: "https://www.canyon.com/sitemap-en_DE.xml" },
    { locale: "de-de", url: "https://www.canyon.com/sitemap-de_DE.xml" },
  ]);
});
