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

Install Rust 1.96 or newer and a C toolchain for the bundled SQLite, then build and install both binaries with Cargo.

```sh
git clone https://github.com/jb381/canyon-mcp.git
cd canyon-mcp
cargo install --path . --locked
canyon search grail --limit 5
```

Pick a model ID from the results to see sizes, colors, specifications, and variant availability:

```sh
canyon product 4509
```

The default store is `en-de`. Use `--locale de-de` or another Canyon language-country locale to change it. Run `canyon locales` to see locales found in Canyon's sitemap.

### A few useful routes

```sh
# Find listed medium Grail models with stock in the selected store
canyon search grail --size M --in-stock

# Compare models and get machine-readable output
canyon compare 4509 4508
canyon search grail --json

# Export a shortlist, or check a model periodically
canyon export --world gravel --format csv --output gravel.csv
canyon watch 4509 --size M --interval 300
```

`watch` prints an initial snapshot and changes in variants, price, or availability. Stop it with Ctrl-C. Its minimum polling interval is 60 seconds. Run `canyon --help` for every command and option.

## Connect an MCP client

Use `command -v canyon-mcp` to find the server binary and enter its absolute path in the MCP client configuration.

```json
{
  "mcpServers": {
    "canyon": {
      "command": "/absolute/path/to/canyon-mcp"
    }
  }
}
```

The server uses stdio and reserves stdout for protocol messages.

Ask your client to *search for a gravel bike in size M*, *compare two model IDs*, or *show the variants for a product URL*. The four tools return human-readable text and structured data.

## Data, limits, and courtesy

- Search reads at most the first **200** listing tiles per request. It is a shortlist tool, not a complete catalog mirror. `total` is best effort and may differ from Canyon's full catalog count.
- Search tiles can omit or leave availability unknown. Use `product` or `get_bike` to inspect variant-level stock. `--in-stock` filters the listing, but a final product-page check is still sensible.
- Results include `fetchedAt` and `stale`. `stale: true` means a temporary request failure caused a fallback to older cached data.
- Successful responses are cached in `~/.cache/canyon-mcp/cache.sqlite`. Within each CLI or MCP process, upstream requests are spaced at least two seconds apart by default. Set `CANYON_CACHE_PATH` to change the cache location, `CANYON_MIN_INTERVAL_MS` to change the delay, and `CANYON_USER_AGENT` to identify a shared deployment.
- The HTTP client accepts only `https://www.canyon.com` pages and blocks `/on/demandware.store/` endpoints. Please keep usage modest and respect Canyon's [robots.txt](https://www.canyon.com/robots.txt) and website terms.

## Docker

```sh
docker compose up -d --build
docker compose ps
docker exec canyon-mcp canyon cache
```

Configure a container-backed MCP client with `"command": "docker"` and `"args": ["exec", "-i", "canyon-mcp", "canyon-mcp"]`. Compose keeps the container available for stdio sessions and mounts a persistent cache at `/data`. The SQLite schema and container UID/GID 1000 are compatible with the previous Bun deployment. Existing `docker exec -i canyon-mcp bun /app/dist/mcp.js` configurations launch the same Rust server through a compatibility wrapper.

## Develop

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo package --locked
```

Tests use local fixtures. For an occasional live smoke test, run `canyon search Grail --locale en-de --limit 3 --json --refresh`. See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution checklist.

## License

[MIT](LICENSE). Canyon and its product names are trademarks of their respective owners.
