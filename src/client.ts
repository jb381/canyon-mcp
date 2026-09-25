import { CanyonError } from "./errors.js";
import { CanyonHttpClient, assertAllowedCanyonUrl } from "./http.js";
import { DEFAULT_LOCALE, localeForSitemap, normalizeLocale } from "./locale.js";
import {
  parseAllBikesUrl,
  parseProductPage,
  parseSearchFilterLinks,
  parseSearchPage,
  parseSitemapIndex,
  parseUrlSet,
} from "./parser.js";
import type { BikeDetail, SearchOptions, SearchResult } from "./types.js";

const SEARCH_CACHE_MS = 30 * 60 * 1000;
const PRODUCT_CACHE_MS = 10 * 60 * 1000;
const SITEMAP_CACHE_MS = 24 * 60 * 60 * 1000;
const HOME_CACHE_MS = 24 * 60 * 60 * 1000;

const SORT_RULES: Record<NonNullable<SearchOptions["sort"]>, string> = {
  availability: "sort_master_availability",
  "price-asc": "sort_price_ascending",
  "price-desc": "sort_price_descending",
  newest: "sort_last_added",
  bestseller: "bestsellers",
};

const WORLD_VALUES = new Map([
  ["mountain", "Mountain"],
  ["road", "Road"],
  ["e-bike", "E-Bike"],
  ["ebike", "E-Bike"],
  ["gravel", "Gravel"],
  ["city/touring", "City/Touring"],
  ["city", "City/Touring"],
  ["triathlon", "Triathlon"],
]);

export interface CanyonClientOptions {
  http?: CanyonHttpClient;
}

function searchUrl(options: SearchOptions): URL {
  const locale = normalizeLocale(options.locale ?? DEFAULT_LOCALE);
  const url = new URL(
    `https://www.canyon.com/${locale}/${options.query?.trim() ? "shop" : "lp/bicycle"}/`,
  );
  if (options.query?.trim()) {
    url.searchParams.set("q", options.query.trim());
    url.searchParams.set("searchType", "bikes");
  }
  const refinements: Array<[string, string]> = [];
  if (options.inStock) refinements.push(["isInStock", "In-stock"]);
  if (options.size?.trim()) refinements.push(["pc_rahmengroesse", options.size.trim().toUpperCase()]);
  if (options.world?.trim()) {
    const world = WORLD_VALUES.get(options.world.trim().toLowerCase());
    if (!world) {
      throw new CanyonError(
        `Unknown world '${options.world}'. Use mountain, road, e-bike, gravel, city/touring, or triathlon.`,
        "INVALID_WORLD",
      );
    }
    refinements.push(["pc_welt", world]);
  }
  refinements.forEach(([name, value], index) => {
    const number = index + 1;
    url.searchParams.set(`prefn${number}`, name);
    url.searchParams.set(`prefv${number}`, value);
  });
  url.searchParams.set("srule", SORT_RULES[options.sort ?? "availability"]);
  url.searchParams.set("start", "0");
  url.searchParams.set("sz", "200");
  return url;
}

function identifierLooksLikeUrl(identifier: string): boolean {
  return /^https?:\/\//i.test(identifier);
}

export class CanyonClient {
  private readonly http: CanyonHttpClient;

  constructor(options: CanyonClientOptions = {}) {
    this.http = options.http ?? new CanyonHttpClient();
  }

  async search(options: SearchOptions = {}): Promise<SearchResult> {
    const locale = normalizeLocale(options.locale ?? DEFAULT_LOCALE);
    const baseUrl = await this.searchBaseUrl({ ...options, locale });
    const baseResponse = await this.http.get(baseUrl, options.refresh ? 0 : SEARCH_CACHE_MS);
    const response = await this.applyFilters(baseResponse, options);
    if (!response) {
      return {
        products: [],
        total: 0,
        locale,
        url: baseResponse.url,
        fetchedAt: baseResponse.fetchedAt,
        stale: baseResponse.stale,
      };
    }
    const parsed = parseSearchPage(response.body, response.url, Boolean(options.inStock));
    const products = options.includeGear ? parsed.products : parsed.products.filter(isBikeSummary);
    const limit = Math.min(Math.max(options.limit ?? 25, 1), 200);
    return {
      products: products.slice(0, limit),
      total: options.includeGear ? parsed.total : Math.min(parsed.total, products.length),
      locale,
      url: response.url,
      fetchedAt: response.fetchedAt,
      stale: response.stale,
    };
  }

  async getProduct(
    identifier: string,
    options: { locale?: string; refresh?: boolean } = {},
  ): Promise<BikeDetail> {
    const locale = normalizeLocale(options.locale ?? DEFAULT_LOCALE);
    const url = identifierLooksLikeUrl(identifier)
      ? assertAllowedCanyonUrl(identifier).toString()
      : await this.resolveProductUrl(identifier, locale, options.refresh ?? false);
    const response = await this.http.get(url, options.refresh ? 0 : PRODUCT_CACHE_MS);
    return parseProductPage(response.body, response.url, response.fetchedAt, response.stale);
  }

  async compare(
    identifiers: string[],
    options: { locale?: string; refresh?: boolean } = {},
  ): Promise<BikeDetail[]> {
    if (identifiers.length < 2 || identifiers.length > 5) {
      throw new CanyonError("Compare requires between 2 and 5 product identifiers.", "INVALID_COUNT");
    }
    const results: BikeDetail[] = [];
    for (const identifier of identifiers) results.push(await this.getProduct(identifier, options));
    return results;
  }

  async listLocales(refresh = false): Promise<Array<{ locale: string; url: string }>> {
    const response = await this.http.get(
      "https://www.canyon.com/sitemap_index.xml",
      refresh ? 0 : SITEMAP_CACHE_MS,
    );
    return parseSitemapIndex(response.body);
  }

  async listProductUrls(locale = DEFAULT_LOCALE, refresh = false): Promise<string[]> {
    const normalized = normalizeLocale(locale);
    const url = `https://www.canyon.com/sitemap-${localeForSitemap(normalized)}.xml`;
    const response = await this.http.get(url, refresh ? 0 : SITEMAP_CACHE_MS);
    return parseUrlSet(response.body).filter((entry) => /\/\d+\.html(?:\?|$)/.test(entry));
  }

  cacheStats(): { path: string; entries: number; bytes: number } {
    return this.http.cacheStats();
  }

  private async resolveProductUrl(identifier: string, locale: string, refresh: boolean): Promise<string> {
    const result = await this.search({ query: identifier, locale, limit: 200, refresh });
    const exact = result.products.find(
      (product) => product.modelId === identifier || product.sku === identifier,
    );
    const candidate = exact ?? (result.products.length === 1 ? result.products[0] : undefined);
    if (!candidate) {
      throw new CanyonError(
        `Could not resolve '${identifier}' to one Canyon product. Search by name first or pass a full product URL.`,
        "PRODUCT_NOT_FOUND",
      );
    }
    return candidate.url;
  }

  private async searchBaseUrl(options: SearchOptions & { locale: string }): Promise<URL> {
    if (options.query?.trim()) {
      const url = new URL(`https://www.canyon.com/${options.locale}/shop/`);
      url.searchParams.set("q", options.query.trim());
      url.searchParams.set("searchType", "bikes");
      url.searchParams.set("srule", SORT_RULES[options.sort ?? "availability"]);
      url.searchParams.set("start", "0");
      url.searchParams.set("sz", "200");
      return url;
    }
    const home = await this.http.get(`https://www.canyon.com/${options.locale}/`, HOME_CACHE_MS);
    const catalogUrl = parseAllBikesUrl(home.body, home.url);
    if (!catalogUrl) {
      throw new CanyonError(`Could not find Canyon's all-bikes page for locale ${options.locale}.`, "CATALOG_NOT_FOUND");
    }
    const url = new URL(catalogUrl);
    url.searchParams.set("srule", SORT_RULES[options.sort ?? "availability"]);
    url.searchParams.set("start", "0");
    url.searchParams.set("sz", "200");
    return url;
  }

  private async applyFilters(
    baseResponse: Awaited<ReturnType<CanyonHttpClient["get"]>>,
    options: SearchOptions,
  ): Promise<Awaited<ReturnType<CanyonHttpClient["get"]>> | undefined> {
    const desired: Array<{ name: string; canonicalValue: string }> = [];
    if (options.inStock) desired.push({ name: "isInStock", canonicalValue: "In-stock" });
    if (options.size?.trim()) desired.push({ name: "pc_rahmengroesse", canonicalValue: options.size.trim().toUpperCase() });
    if (options.world?.trim()) {
      const world = WORLD_VALUES.get(options.world.trim().toLowerCase());
      if (!world) {
        throw new CanyonError(
          `Unknown world '${options.world}'. Use mountain, road, e-bike, gravel, city/touring, or triathlon.`,
          "INVALID_WORLD",
        );
      }
      desired.push({ name: "pc_welt", canonicalValue: world });
    }
    if (desired.length === 0) return baseResponse;
    const links = parseSearchFilterLinks(baseResponse.body, baseResponse.url);
    const url = new URL(baseResponse.url);
    for (let index = 0; index < desired.length; index += 1) {
      const refinement = desired[index]!;
      const link = links.find(
        (candidate) =>
          candidate.name === refinement.name &&
          candidate.canonicalValue.toLowerCase() === refinement.canonicalValue.toLowerCase(),
      );
      if (!link) {
        return undefined;
      }
      const number = index + 1;
      url.searchParams.set(`prefn${number}`, refinement.name);
      url.searchParams.set(`prefv${number}`, link.actualValue);
    }
    return this.http.get(url, options.refresh ? 0 : SEARCH_CACHE_MS);
  }
}

function isBikeSummary(product: { category?: string; subcategory?: string; outlet?: boolean }): boolean {
  const category = product.category?.toLowerCase() ?? "";
  const subcategory = product.subcategory?.toLowerCase() ?? "";
  const bikeTerms = ["bike", "road", "gravel", "mountain", "e-bike", "city", "touring", "triathlon", "hybrid", "race", "endurance"];
  return category.includes("bike") ||
    (category === "outlet" && product.outlet === true && bikeTerms.some((term) => subcategory.includes(term)));
}

export { searchUrl };
