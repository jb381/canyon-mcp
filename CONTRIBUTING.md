# Contributing

Thanks for helping make bicycle research a little less tab-heavy.

1. Open an issue for significant changes or a new data source so the approach can be discussed first.
2. Keep requests limited to public pages, preserve the existing request spacing, and avoid checkout or account endpoints.
3. Add a focused local fixture or test when changing a parser or network behavior. Do not make the default test suite depend on Canyon being online.
4. Run `bun run check`, `bun run test`, and `bun run build` before opening a pull request.

Catalog HTML changes over time. If a page stops parsing, include the affected URL, locale, observed behavior, and a small sanitized fixture when filing an issue. Please remove any account or personal information from reports.
