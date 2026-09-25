import assert from "node:assert/strict";
import { test } from "node:test";
import { searchUrl } from "../src/client.js";
import { assertAllowedCanyonUrl } from "../src/http.js";

test("builds public search URLs with numbered refinements", () => {
  const url = searchUrl({
    query: "grail",
    locale: "en-de",
    inStock: true,
    size: "m",
    world: "gravel",
    sort: "price-asc",
  });
  assert.equal(url.pathname, "/en-de/shop/");
  assert.equal(url.searchParams.get("q"), "grail");
  assert.equal(url.searchParams.get("prefn1"), "isInStock");
  assert.equal(url.searchParams.get("prefn2"), "pc_rahmengroesse");
  assert.equal(url.searchParams.get("prefv2"), "M");
  assert.equal(url.searchParams.get("prefn3"), "pc_welt");
  assert.equal(url.searchParams.get("srule"), "sort_price_ascending");
  assert.equal(url.searchParams.get("sz"), "200");
});

test("uses the search route for query searches", () => {
  const url = searchUrl({ query: "grail", locale: "de-de" });
  assert.equal(url.pathname, "/de-de/shop/");
  assert.equal(url.searchParams.get("searchType"), "bikes");
});

test("blocks non-Canyon hosts and robots-disallowed commerce endpoints", () => {
  assert.throws(() => assertAllowedCanyonUrl("https://example.com/product"), /Refusing non-Canyon/);
  assert.throws(
    () => assertAllowedCanyonUrl("https://www.canyon.com/on/demandware.store/Sites-RoW-Site/en_DE/Product-Variation"),
    /disallowed by robots/,
  );
  assert.equal(
    assertAllowedCanyonUrl("https://www.canyon.com/en-de/lp/bicycle/#top").hash,
    "",
  );
});
