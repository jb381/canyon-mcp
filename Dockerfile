# syntax=docker/dockerfile:1
FROM rust:1.96-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bins && \
    install -Dm755 target/release/canyon /out/canyon && \
    install -Dm755 target/release/canyon-mcp /out/canyon-mcp

FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 1000 canyon && useradd --uid 1000 --gid canyon --create-home canyon && \
    install -d -o canyon -g canyon /data
ENV CANYON_CACHE_PATH=/data/cache.sqlite \
    CANYON_MIN_INTERVAL_MS=2000
COPY --from=build /out/ /usr/local/bin/
COPY --chmod=755 docker/bun /usr/local/bin/bun
USER canyon
CMD ["sleep", "infinity"]
