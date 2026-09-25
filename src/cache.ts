import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { Database } from "bun:sqlite";

export interface CacheEntry {
  url: string;
  body: string;
  etag?: string;
  lastModified?: string;
  fetchedAt: string;
}

interface CacheRow {
  url: string;
  body: string;
  etag: string | null;
  last_modified: string | null;
  fetched_at: string;
}

export function defaultCachePath(): string {
  return process.env.CANYON_CACHE_PATH ?? join(homedir(), ".cache", "canyon-mcp", "cache.sqlite");
}

export class HttpCache {
  readonly path: string;
  private readonly database: Database;

  constructor(path = defaultCachePath()) {
    this.path = path;
    if (path !== ":memory:") mkdirSync(dirname(path), { recursive: true });
    this.database = new Database(path);
    this.database.run(`
      PRAGMA journal_mode = WAL;
      CREATE TABLE IF NOT EXISTS http_cache (
        url TEXT PRIMARY KEY,
        body TEXT NOT NULL,
        etag TEXT,
        last_modified TEXT,
        fetched_at TEXT NOT NULL
      );
    `);
  }

  get(url: string): CacheEntry | undefined {
    const row = this.database
      .query("SELECT url, body, etag, last_modified, fetched_at FROM http_cache WHERE url = ?")
      .get(url) as unknown as CacheRow | undefined;
    if (!row) return undefined;
    return {
      url: row.url,
      body: row.body,
      fetchedAt: row.fetched_at,
      ...(row.etag ? { etag: row.etag } : {}),
      ...(row.last_modified ? { lastModified: row.last_modified } : {}),
    };
  }

  put(entry: CacheEntry): void {
    this.database
      .query(`
        INSERT INTO http_cache (url, body, etag, last_modified, fetched_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(url) DO UPDATE SET
          body = excluded.body,
          etag = excluded.etag,
          last_modified = excluded.last_modified,
          fetched_at = excluded.fetched_at
      `)
      .run(entry.url, entry.body, entry.etag ?? null, entry.lastModified ?? null, entry.fetchedAt);
  }

  touch(url: string, fetchedAt: string): void {
    this.database.query("UPDATE http_cache SET fetched_at = ? WHERE url = ?").run(fetchedAt, url);
  }

  stats(): { path: string; entries: number; bytes: number } {
    const row = this.database
      .query("SELECT COUNT(*) AS entries, COALESCE(SUM(LENGTH(body)), 0) AS bytes FROM http_cache")
      .get() as unknown as { entries: number; bytes: number };
    return { path: this.path, entries: Number(row.entries), bytes: Number(row.bytes) };
  }

  close(): void {
    this.database.close();
  }
}
