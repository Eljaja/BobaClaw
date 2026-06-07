# syntax=docker/dockerfile:1
# BobaClaw gateway + Docker CLI (sandbox + Obscura MCP spawn sibling containers via host socket).
#
# Build:  make docker-build
# Run:    make docker-up

FROM rust:1-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
COPY workspace-examples ./workspace-examples

RUN cargo build --release -p bobaclaw

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        docker.io \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/bobaclaw /usr/local/bin/bobaclaw
COPY workspace-examples /app/workspace-examples
COPY docker/config.docker.yaml /etc/bobaclaw/config.docker.yaml
COPY docker/entrypoint.sh /usr/local/bin/bobaclaw-entrypoint.sh

RUN chmod +x /usr/local/bin/bobaclaw-entrypoint.sh

ENV BOBACLAW_HOME=/data
VOLUME /data
EXPOSE 18790

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -fsS http://127.0.0.1:18790/health || exit 1

ENTRYPOINT ["/usr/local/bin/bobaclaw-entrypoint.sh"]
