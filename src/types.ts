export type Availability =
  | "in_stock"
  | "out_of_stock"
  | "preorder"
  | "backorder"
  | "limited"
  | "unknown";

export interface Money {
  amount: number;
  currency: string;
}

export type PropertyValue = string | number | boolean | Array<string | number | boolean>;

export interface BikeSummary {
  modelId: string;
  sku?: string;
  name: string;
  url: string;
  category?: string;
  subcategory?: string;
  color?: string;
  highlight?: string;
  image?: string;
  price?: Money;
  availability: Availability;
  outlet?: boolean;
}

export interface BikeVariant {
  sku: string;
  name: string;
  url: string;
  color?: string;
  size?: string;
  image?: string;
  price?: Money;
  availability: Availability;
  attributes: Record<string, PropertyValue>;
}

export interface BikeDetail {
  modelId: string;
  name: string;
  url: string;
  description?: string;
  brand?: string;
  image?: string;
  attributes: Record<string, PropertyValue>;
  variants: BikeVariant[];
  fetchedAt: string;
  stale: boolean;
}

export interface SearchOptions {
  query?: string;
  locale?: string;
  world?: string;
  size?: string;
  inStock?: boolean;
  sort?: "availability" | "price-asc" | "price-desc" | "newest" | "bestseller";
  limit?: number;
  refresh?: boolean;
  includeGear?: boolean;
}

export interface SearchResult {
  products: BikeSummary[];
  total: number;
  locale: string;
  url: string;
  fetchedAt: string;
  stale: boolean;
}

export interface FetchResult {
  body: string;
  url: string;
  fetchedAt: string;
  fromCache: boolean;
  stale: boolean;
}
