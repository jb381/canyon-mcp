import { load, type Cheerio } from "cheerio";
import type { AnyNode } from "domhandler";
import { ParseError } from "./errors.js";
import type {
  Availability,
  BikeDetail,
  BikeSummary,
  BikeVariant,
  Money,
  PropertyValue,
} from "./types.js";

type JsonObject = Record<string, unknown>;

function asObject(value: unknown): JsonObject | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonObject)
    : undefined;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function firstString(value: unknown): string | undefined {
  if (Array.isArray(value)) return asString(value[0]);
  return asString(value);
}

function absoluteCanyonUrl(value: string, baseUrl: string): string {
  const url = new URL(value, baseUrl);
  url.hash = "";
  return url.toString();
}

export function normalizeAvailability(value: unknown): Availability {
  const text = String(value ?? "").toLowerCase().replaceAll(/[^a-z]/g, "");
  if (text.includes("instock")) return "in_stock";
  if (text.includes("outofstock")) return "out_of_stock";
  if (text.includes("preorder")) return "preorder";
  if (text.includes("backorder")) return "backorder";
  if (text.includes("limited")) return "limited";
  return "unknown";
}

function parseMoney(offerValue: unknown): Money | undefined {
  const offer = Array.isArray(offerValue) ? asObject(offerValue[0]) : asObject(offerValue);
  if (!offer) return undefined;
  const amount = Number(offer.price);
  const currency = asString(offer.priceCurrency);
  return Number.isFinite(amount) && currency ? { amount, currency } : undefined;
}

function addProperty(target: Record<string, PropertyValue>, name: string, value: PropertyValue): void {
  const existing = target[name];
  if (existing === undefined) {
    target[name] = value;
  } else if (Array.isArray(existing)) {
    target[name] = [...existing, ...(Array.isArray(value) ? value : [value])];
  } else {
    target[name] = [existing, ...(Array.isArray(value) ? value : [value])];
  }
}

function parseProperties(value: unknown): Record<string, PropertyValue> {
  const properties: Record<string, PropertyValue> = {};
  if (!Array.isArray(value)) return properties;
  for (const entry of value) {
    const item = asObject(entry);
    const name = asString(item?.name);
    const raw = item?.value;
    if (!name || !["string", "number", "boolean"].includes(typeof raw) && !Array.isArray(raw)) continue;
    const unit = asString(item?.unitText);
    const normalized = Array.isArray(raw)
      ? raw.filter((part): part is string | number | boolean =>
          ["string", "number", "boolean"].includes(typeof part),
        )
      : (raw as string | number | boolean);
    const withUnit = unit && !Array.isArray(normalized) ? `${normalized} ${unit}` : normalized;
    addProperty(properties, name, withUnit);
  }
  return properties;
}

function findTypedNode(value: unknown, wanted: string): JsonObject | undefined {
  if (Array.isArray(value)) {
    for (const child of value) {
      const found = findTypedNode(child, wanted);
      if (found) return found;
    }
    return undefined;
  }
  const object = asObject(value);
  if (!object) return undefined;
  const types = Array.isArray(object["@type"]) ? object["@type"] : [object["@type"]];
  if (types.includes(wanted)) return object;
  for (const child of Object.values(object)) {
    const found = findTypedNode(child, wanted);
    if (found) return found;
  }
  return undefined;
}

function parseVariant(value: unknown, baseUrl: string): BikeVariant | undefined {
  const variant = asObject(value);
  if (!variant) return undefined;
  const sku = asString(variant.sku);
  const name = asString(variant.name);
  const url = asString(variant.url);
  if (!sku || !name || !url) return undefined;
  const offer = Array.isArray(variant.offers) ? asObject(variant.offers[0]) : asObject(variant.offers);
  return {
    sku,
    name,
    url: absoluteCanyonUrl(url, baseUrl),
    availability: normalizeAvailability(offer?.availability),
    attributes: parseProperties(variant.additionalProperty),
    ...(asString(variant.color) ? { color: asString(variant.color)! } : {}),
    ...(asString(variant.size) ? { size: asString(variant.size)! } : {}),
    ...(firstString(variant.image) ? { image: firstString(variant.image)! } : {}),
    ...(parseMoney(variant.offers) ? { price: parseMoney(variant.offers)! } : {}),
  };
}

export function parseProductPage(
  html: string,
  pageUrl: string,
  fetchedAt = new Date().toISOString(),
  stale = false,
): BikeDetail {
  const $ = load(html);
  const documents: unknown[] = [];
  $('script[type="application/ld+json"]').each((_index, element) => {
    const source = $(element).text().trim();
    if (!source) return;
    try {
      documents.push(JSON.parse(source));
    } catch {
      // Other unrelated JSON-LD blocks should not prevent product extraction.
    }
  });
  const group = documents.map((document) => findTypedNode(document, "ProductGroup")).find(Boolean);
  const product = group ?? documents.map((document) => findTypedNode(document, "Product")).find(Boolean);
  if (!product) throw new ParseError(`No Product or ProductGroup JSON-LD found at ${pageUrl}`);

  const modelId = asString(product.productGroupID) ?? asString(product.sku);
  const name = asString(product.name);
  if (!modelId || !name) throw new ParseError(`Product data at ${pageUrl} is missing an ID or name`);
  const canonical = asString(product.url) ?? pageUrl;
  const brand = asObject(product.brand);
  const rawVariants = Array.isArray(product.hasVariant) ? product.hasVariant : [];
  const variants = rawVariants
    .map((variant) => parseVariant(variant, canonical))
    .filter((variant): variant is BikeVariant => Boolean(variant));

  if (!group && asString(product.sku) && variants.length === 0) {
    const parsed = parseVariant(product, canonical);
    if (parsed) variants.push(parsed);
  }

  return {
    modelId,
    name,
    url: absoluteCanyonUrl(canonical, pageUrl),
    attributes: parseProperties(product.additionalProperty),
    variants,
    fetchedAt,
    stale,
    ...(asString(product.description) ? { description: asString(product.description)! } : {}),
    ...(asString(brand?.name) ? { brand: asString(brand?.name)! } : {}),
    ...(firstString(product.image) ? { image: firstString(product.image)! } : {}),
  };
}

function parseTileJson(raw: string | undefined): JsonObject | undefined {
  if (!raw) return undefined;
  try {
    const parsed = JSON.parse(raw) as unknown;
    const root = Array.isArray(parsed) ? asObject(parsed[0]) : asObject(parsed);
    const ecommerce = asObject(root?.ecommerce);
    const items = Array.isArray(ecommerce?.items) ? ecommerce.items : [];
    return asObject(items[0]);
  } catch {
    return undefined;
  }
}

function tileImage(raw: string | undefined): string | undefined {
  if (!raw) return undefined;
  try {
    const entries = JSON.parse(raw) as unknown;
    const entry = Array.isArray(entries) ? asObject(entries[0]) : undefined;
    const urls = asObject(entry?.urls);
    return asString(urls?.xxl) ?? asString(urls?.xl) ?? asString(urls?.zoom);
  } catch {
    return undefined;
  }
}

function parseLocalizedAmount(raw: string): number | undefined {
  let value = raw.replaceAll(/[\s\u00a0'’]/g, "").replaceAll(/[^\d.,]/g, "");
  if (!value) return undefined;
  const lastDot = value.lastIndexOf(".");
  const lastComma = value.lastIndexOf(",");
  if (lastDot >= 0 && lastComma >= 0) {
    const decimalSeparator = lastDot > lastComma ? "." : ",";
    const thousandsSeparator = decimalSeparator === "." ? "," : ".";
    value = value.replaceAll(thousandsSeparator, "").replace(decimalSeparator, ".");
  } else {
    const separator = lastDot >= 0 ? "." : lastComma >= 0 ? "," : undefined;
    if (separator) {
      const occurrences = [...value].filter((character) => character === separator).length;
      const decimals = value.length - value.lastIndexOf(separator) - 1;
      if (occurrences === 1 && decimals > 0 && decimals <= 2) {
        value = value.replace(separator, ".");
      } else {
        value = value.replaceAll(separator, "");
      }
    }
  }
  const amount = Number(value);
  return Number.isFinite(amount) && amount > 0 ? amount : undefined;
}

function parseTilePrice(tile: Cheerio<AnyNode>, item: JsonObject | undefined): number | undefined {
  const raw = item?.gross_price;
  if (typeof raw === "number" && Number.isFinite(raw) && raw > 0) return raw;
  if (typeof raw === "string" && raw.trim()) {
    const numeric = Number(raw);
    if (Number.isFinite(numeric) && numeric > 0) return numeric;
  }
  const visiblePrice = tile.find(".productTile__priceSale").first().text().trim();
  return visiblePrice ? parseLocalizedAmount(visiblePrice) : undefined;
}

export function parseSearchPage(
  html: string,
  pageUrl: string,
  assumeInStock = false,
): { products: BikeSummary[]; total: number } {
  const $ = load(html);
  const products: BikeSummary[] = [];
  $('[data-product-tile-context="grid"][data-pid]').each((_index, element) => {
    const tile = $(element);
    const item = parseTileJson(tile.attr("data-gtm-impression"));
    const link = tile.find("a.js-productTileLink[href]").first();
    const href = link.attr("href");
    const modelId = asString(item?.item_id);
    const name = asString(item?.item_name) ?? link.attr("title")?.trim();
    if (!href || !modelId || !name) return;
    const grossPrice = parseTilePrice(tile, item);
    const currency = asString(item?.currency);
    const availability = assumeInStock ? "in_stock" : normalizeAvailability(item?.item_availability);
    const selectedSwatch = tile.find(".js-colorSwatchSelected").first();
    const image =
      tileImage(selectedSwatch.attr("data-tile-images")) ??
      tileImage(tile.find("[data-tile-images]").first().attr("data-tile-images"));
    products.push({
      modelId,
      sku: tile.attr("data-pid")!,
      name,
      url: absoluteCanyonUrl(href, pageUrl),
      availability,
      ...(asString(item?.item_category) ? { category: asString(item?.item_category)! } : {}),
      ...(asString(item?.item_category2) ? { subcategory: asString(item?.item_category2)! } : {}),
      ...(asString(item?.item_color) && item?.item_color !== "not defined"
        ? { color: asString(item?.item_color)! }
        : {}),
      ...(grossPrice !== undefined && currency
        ? { price: { amount: grossPrice, currency } }
        : {}),
      ...(item?.item_outlet_flag === "true" ? { outlet: true } : {}),
      ...(tile.find(".productTileDefault__info--highlights").text().trim()
        ? { highlight: tile.find(".productTileDefault__info--highlights").text().trim() }
        : {}),
      ...(image ? { image } : {}),
    });
  });

  const countText = $(".js-productCount").first().text().trim();
  const bodyText = $("body").text().replaceAll(/\s+/g, " ");
  const countMatch = countText.match(/[\d.,\s]+/) ?? bodyText.match(/\b[\d.,\s]+\s+(?:items|Artikel|articles)\b/i);
  const count = countMatch?.[0]?.replaceAll(/[^\d]/g, "");
  return { products, total: count ? Number.parseInt(count, 10) : products.length };
}

export interface SearchFilterLink {
  name: string;
  canonicalValue: string;
  actualValue: string;
}

export function parseSearchFilterLinks(html: string, pageUrl: string): SearchFilterLink[] {
  const $ = load(html);
  const links: SearchFilterLink[] = [];
  $("button.js-productFilterButton[data-url][data-gtm-click]").each((_index, element) => {
    const button = $(element);
    const rawEvent = button.attr("data-gtm-click");
    const dataUrl = button.attr("data-url");
    if (!rawEvent || !dataUrl) return;
    try {
      const eventData = JSON.parse(rawEvent) as unknown;
      const event = Array.isArray(eventData) ? asObject(eventData[0]) : undefined;
      const name = asString(event?.filter_name);
      const canonicalValue = asString(event?.filter_value);
      if (!name || !canonicalValue) return;
      const url = new URL(absoluteCanyonUrl(dataUrl, pageUrl));
      let actualValue: string | undefined;
      for (let number = 1; number <= 5; number += 1) {
        if (url.searchParams.get(`prefn${number}`) === name) {
          actualValue = url.searchParams.get(`prefv${number}`) ?? undefined;
          break;
        }
      }
      if (actualValue) links.push({ name, canonicalValue, actualValue });
    } catch {
      // A malformed analytics attribute must not prevent the other filters from working.
    }
  });
  return links;
}

export function parseAllBikesUrl(html: string, pageUrl: string): string | undefined {
  const $ = load(html);
  const href = $("a[data-eventlabel='All Bikes'][href]").first().attr("href");
  if (href) return absoluteCanyonUrl(href, pageUrl);
  const fallback = $("a[href]")
    .map((_index, element) => ({ href: $(element).attr("href"), text: $(element).text() }))
    .get()
    .find((entry) => entry.href && /\/lp\/[^/]+\/?(?:\?|$)/.test(entry.href) && /bike|fahrr|velo/i.test(`${entry.href} ${entry.text}`));
  return fallback?.href ? absoluteCanyonUrl(fallback.href, pageUrl) : undefined;
}

export function parseSitemapIndex(xml: string): Array<{ locale: string; url: string }> {
  const $ = load(xml, { xmlMode: true });
  const entries: Array<{ locale: string; url: string }> = [];
  $("sitemap > loc").each((_index, element) => {
    const url = $(element).text().trim();
    const match = url.match(/sitemap-([a-z]{2})_([A-Z]{2})\.xml$/);
    if (match) entries.push({ locale: `${match[1]}-${match[2]!.toLowerCase()}`, url });
  });
  return entries;
}

export function parseUrlSet(xml: string): string[] {
  const $ = load(xml, { xmlMode: true });
  return $("url > loc")
    .map((_index, element) => $(element).text().trim())
    .get()
    .filter(Boolean);
}
