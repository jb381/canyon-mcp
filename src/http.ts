import { HttpCache } from "./cache.js";
import { CanyonError } from "./errors.js";
import type { FetchResult } from "./types.js";

const DEFAULT_USER_AGENT =
  "canyon-mcp/0.1 (+https://github.com/jb381/canyon-mcp)";

export interface CanyonHttpClientOptions {
  cache?: HttpCache;
  minIntervalMs?: number;
  timeoutMs?: number;
  userAgent?: string;
  fetchImpl?: typeof fetch;
}

function wait(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function isRedirect(status: number): boolean {
  return [301, 302, 303, 307, 308].includes(status);
}

export function assertAllowedCanyonUrl(value: string | URL): URL {
  const url = value instanceof URL ? new URL(value) : new URL(value);
  if (url.protocol !== "https:" || url.hostname !== "www.canyon.com") {
    throw new CanyonError(`Refusing non-Canyon URL: ${url.toString()}`, "URL_NOT_ALLOWED");
  }
  if (url.pathname.startsWith("/on/demandware.store/")) {
    throw new CanyonError(
      `Refusing Canyon endpoint disallowed by robots.txt: ${url.pathname}`,
      "ROBOTS_DISALLOWED",
    );
  }
  url.hash = "";
  return url;
}

export class CanyonHttpClient {
  private readonly cache: HttpCache;
  private readonly minIntervalMs: number;
  private readonly timeoutMs: number;
  private readonly userAgent: string;
  private readonly fetchImpl: typeof fetch;
  private nextRequestAt = 0;
  private requestQueue: Promise<void> = Promise.resolve();

  constructor(options: CanyonHttpClientOptions = {}) {
    this.cache = options.cache ?? new HttpCache();
    const configuredInterval = options.minIntervalMs ?? Number(process.env.CANYON_MIN_INTERVAL_MS ?? 2000);
    this.minIntervalMs = Number.isFinite(configuredInterval) && configuredInterval >= 0
      ? configuredInterval
      : 2000;
    this.timeoutMs = options.timeoutMs ?? 20_000;
    this.userAgent = options.userAgent ?? process.env.CANYON_USER_AGENT ?? DEFAULT_USER_AGENT;
    this.fetchImpl = options.fetchImpl ?? fetch;
  }

  async get(value: string | URL, maxAgeMs: number): Promise<FetchResult> {
    const url = assertAllowedCanyonUrl(value);
    const key = url.toString();
    const cached = this.cache.get(key);
    const now = Date.now();
    if (cached && now - Date.parse(cached.fetchedAt) <= maxAgeMs) {
      return { ...cached, fromCache: true, stale: false };
    }

    const headers = new Headers({
      accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
      "accept-language": "en-US,en;q=0.8",
      "user-agent": this.userAgent,
    });
    if (cached?.etag) headers.set("if-none-match", cached.etag);
    if (cached?.lastModified) headers.set("if-modified-since", cached.lastModified);

    let lastError: unknown;
    let allowStaleFallback = true;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      try {
        const { response, finalUrl } = await this.fetchAllowed(url, headers);
        if (response.status === 304 && cached) {
          const fetchedAt = new Date().toISOString();
          this.cache.touch(key, fetchedAt);
          return { ...cached, fetchedAt, fromCache: true, stale: false };
        }
        if ((response.status === 429 || response.status >= 500) && attempt < 2) {
          const retryAfter = Number.parseInt(response.headers.get("retry-after") ?? "0", 10) * 1000;
          await response.body?.cancel();
          await wait(Math.max(retryAfter, 1000 * 2 ** attempt));
          continue;
        }
        if (!response.ok) {
          allowStaleFallback = response.status === 429 || response.status >= 500;
          throw new CanyonError(
            `Canyon returned HTTP ${response.status} for ${url.toString()}`,
            "HTTP_ERROR",
          );
        }
        const body = await response.text();
        const fetchedAt = new Date().toISOString();
        this.cache.put({
          url: key,
          body,
          fetchedAt,
          ...(response.headers.get("etag") ? { etag: response.headers.get("etag")! } : {}),
          ...(response.headers.get("last-modified")
            ? { lastModified: response.headers.get("last-modified")! }
            : {}),
        });
        return { body, url: finalUrl, fetchedAt, fromCache: false, stale: false };
      } catch (error) {
        lastError = error;
        if (error instanceof CanyonError && error.code !== "HTTP_ERROR") {
          allowStaleFallback = false;
        }
        if (error instanceof CanyonError || attempt === 2) break;
        await wait(1000 * 2 ** attempt);
      }
    }

    if (cached && allowStaleFallback) return { ...cached, fromCache: true, stale: true };
    if (lastError instanceof CanyonError) throw lastError;
    throw new CanyonError(`Unable to fetch ${url.toString()}`, "NETWORK_ERROR", {
      cause: lastError,
    });
  }

  cacheStats(): { path: string; entries: number; bytes: number } {
    return this.cache.stats();
  }

  private async fetchAllowed(
    initialUrl: URL,
    headers: Headers,
  ): Promise<{ response: Response; finalUrl: string }> {
    let url = initialUrl;
    const requestHeaders = new Headers(headers);
    for (let redirects = 0; redirects <= 5; redirects += 1) {
      await this.throttle();
      const response = await this.fetchImpl(url, {
        headers: requestHeaders,
        redirect: "manual",
        signal: AbortSignal.timeout(this.timeoutMs),
      });
      if (!isRedirect(response.status)) {
        return { response, finalUrl: url.toString() };
      }
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location) {
        throw new CanyonError(`Canyon redirect has no Location header: ${url}`, "INVALID_REDIRECT");
      }
      url = assertAllowedCanyonUrl(new URL(location, url));
      requestHeaders.delete("if-none-match");
      requestHeaders.delete("if-modified-since");
    }
    throw new CanyonError(`Too many redirects for ${initialUrl}`, "TOO_MANY_REDIRECTS");
  }

  private async throttle(): Promise<void> {
    const previous = this.requestQueue;
    let release!: () => void;
    this.requestQueue = new Promise<void>((resolve) => { release = resolve; });
    await previous;
    try {
      const delay = this.nextRequestAt - Date.now();
      if (delay > 0) await wait(delay);
      this.nextRequestAt = Date.now() + this.minIntervalMs;
    } finally {
      release();
    }
  }
}
