# canyon-mcp

**Find the bike. Close the tabs. Skip the spreadsheet.**

A read-only CLI and [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server for Canyon's public bicycle catalog. Search models, compare specifications, inspect sizes and availability, or export a shortlist. No account or checkout flow required.

> [!NOTE]
> This is an independent community project, not an official Canyon product. Prices, specifications, and stock can change. Always check the linked Canyon product page before making a purchase decision.

## What you can do

| Task | CLI | MCP tool |
| --- | --- | --- |
| Find and filter bikes | `search` | `search_bikes` |
| Inspect a model and its variants | `product` | `get_bike` |
| Compare 2–5 models | `compare` | `compare_bikes` |
| Discover supported locales | `locales` | `list_locales` |
| Export search results | `export` | — |
| Watch a model for stock changes | `watch` | — |

The project reads public listing, product, and sitemap pages. It does not sign in, add items to a cart, or place orders. A bike may still sell out between a query and your next ride.

## Quick start

Install [Bun](https://bun.com/docs/installation) 1.3 or newer, then clone this repository:

```sh
git clone https://github.com/jb381/canyon-mcp.git
cd canyon-mcp
bun install --frozen-lockfile
bun run dev:cli search grail --limit 5
```

Pick a model ID from the results to see sizes, colors, specifications, and variant availability:

```sh
bun run dev:cli product 4509
```

The default store is `en-de`. Use `--locale de-de` or another Canyon language-country locale to change it. Run `bun run dev:cli locales` to see locales found in Canyon's sitemap.

### A few useful routes

```sh
# Find listed medium Grail models with stock in the selected store
bun run dev:cli search grail --size M --in-stock

# Compare models and get machine-readable output
bun run dev:cli compare 4509 4508
bun run dev:cli search grail --json

# Export a shortlist, or check a model periodically
bun run dev:cli export --world gravel --format csv --output gravel.csv
bun run dev:cli watch 4509 --size M --interval 300
```

`watch` prints an initial snapshot and changes in variants, price, or availability. Stop it with Ctrl-C. Its minimum polling interval is 60 seconds. Run `bun run dev:cli --help` for every command and option.

## Connect an MCP client

Build once, then point your MCP client at the compiled server using an **absolute path**:

```sh
bun run build
which bun
```

```json
{
  "mcpServers": {
    "canyon": {
      "command": "/absolute/path/to/bun",
      "args": ["/absolute/path/to/canyon-mcp/dist/mcp.js"]
    }
  }
}
```

Replace both example paths with paths on your machine. Some MCP clients can resolve `bun` directly, in which case `"command": "bun"` also works. The server uses stdio; its normal output is reserved for MCP messages.

Ask your client to *search for a gravel bike in size M*, *compare two model IDs*, or *show the variants for a product URL*. The four tools return human-readable text and structured data.

## Data, limits, and courtesy

- Search reads at most the first **200** listing tiles per request. It is a shortlist tool, not a complete catalog mirror. `total` is best effort and may differ from Canyon's full catalog count.
- Search tiles can omit or leave availability unknown. Use `product` or `get_bike` to inspect variant-level stock. `--in-stock` filters the listing, but a final product-page check is still sensible.
- Results include `fetchedAt` and `stale`. `stale: true` means a temporary request failure caused a fallback to older cached data.
- Successful responses are cached in `~/.cache/canyon-mcp/cache.sqlite`. By default, requests are spaced at least two seconds apart. Set `CANYON_CACHE_PATH` to change the cache location, `CANYON_MIN_INTERVAL_MS` to change the delay, and `CANYON_USER_AGENT` to identify a shared deployment.
- The HTTP client accepts only `https://www.canyon.com` pages and blocks `/on/demandware.store/` endpoints. Please keep usage modest and respect Canyon's [robots.txt](https://www.canyon.com/robots.txt) and website terms.

## Develop

```sh
bun run check
bun run test
bun run build
bun pm pack --dry-run
```

Tests use local fixtures. `bun run test:live` makes a fresh request to Canyon and should be run sparingly. See [CONTRIBUTING.md](CONTRIBUTING.md) for the small contribution checklist.

## License

[MIT](LICENSE). Canyon and its product names are trademarks of their respective owners.
