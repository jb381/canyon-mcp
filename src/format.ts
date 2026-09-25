import type { BikeDetail, BikeSummary, PropertyValue } from "./types.js";

export function formatMoney(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat("en", { style: "currency", currency }).format(amount);
  } catch {
    return `${amount} ${currency}`;
  }
}

export function formatSearchTable(products: BikeSummary[]): string {
  if (products.length === 0) return "No bikes found.";
  const rows = products.map((product) => ({
    ID: product.modelId,
    Name: product.name,
    Price: product.price ? formatMoney(product.price.amount, product.price.currency) : "—",
    Stock: product.availability,
    Category: product.category ?? "—",
    URL: product.url,
  }));
  return renderTable(rows);
}

function printable(value: PropertyValue | undefined): string {
  if (value === undefined) return "—";
  return Array.isArray(value) ? value.join(" / ") : String(value);
}

export function formatProduct(detail: BikeDetail): string {
  const available = detail.variants.filter((variant) => variant.availability === "in_stock");
  const lines = [
    `${detail.name} (${detail.modelId})`,
    detail.description ?? "",
    `URL: ${detail.url}`,
    `Variants: ${detail.variants.length}; in stock: ${available.length}`,
    `Weight: ${printable(detail.attributes.Weight)}`,
    `Material: ${printable(detail.attributes.Material)}`,
    `Fetched: ${detail.fetchedAt}${detail.stale ? " (stale cache)" : ""}`,
  ].filter(Boolean);
  if (detail.variants.length > 0) {
    lines.push("", renderTable(detail.variants.map((variant) => ({
      SKU: variant.sku,
      Color: variant.color ?? "—",
      Size: variant.size ?? "—",
      Price: variant.price ? formatMoney(variant.price.amount, variant.price.currency) : "—",
      Stock: variant.availability,
    }))));
  }
  return lines.join("\n");
}

export function formatComparison(details: BikeDetail[]): string {
  const keys = ["Weight", "Material", "World", "Style", "Year", "Rear Derailleur", "Wheel", "Tyre"];
  const rows: Array<Record<string, string>> = keys.map((key) => {
    const row: Record<string, string> = { Attribute: key };
    details.forEach((detail) => {
      row[detail.name] = printable(detail.attributes[key]);
    });
    return row;
  });
  const priceRow: Record<string, string> = { Attribute: "Price" };
  details.forEach((detail) => {
    const price = detail.variants.find((variant) => variant.price)?.price;
    priceRow[detail.name] = price ? formatMoney(price.amount, price.currency) : "—";
  });
  rows.unshift(priceRow);
  return renderTable(rows);
}

export function toCsv(products: BikeSummary[]): string {
  const headers = ["modelId", "sku", "name", "price", "currency", "availability", "category", "url"];
  const escape = (value: unknown): string => {
    const text = String(value ?? "");
    return /[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
  };
  const rows = products.map((product) => [
    product.modelId,
    product.sku,
    product.name,
    product.price?.amount,
    product.price?.currency,
    product.availability,
    product.category,
    product.url,
  ]);
  return [headers, ...rows].map((row) => row.map(escape).join(",")).join("\n") + "\n";
}

function renderTable(rows: Array<Record<string, string>>): string {
  if (rows.length === 0) return "";
  const headers = Object.keys(rows[0]!);
  const widths = headers.map((header) =>
    Math.max(header.length, ...rows.map((row) => (row[header] ?? "").length)),
  );
  const render = (row: Record<string, string>) =>
    headers.map((header, index) => (row[header] ?? "").padEnd(widths[index]!)).join("  ").trimEnd();
  const heading = Object.fromEntries(headers.map((header) => [header, header]));
  return [render(heading), widths.map((width) => "-".repeat(width)).join("  "), ...rows.map(render)].join("\n");
}
