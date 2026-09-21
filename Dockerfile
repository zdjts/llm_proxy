# llm_proxy v2.0.0 — Dockerfile (multi-stage, minimal)
# Build:  docker build -t llm_proxy:v2.0.0 .
# Run:    docker run -p 8080:8080 -v $(pwd)/config.yaml:/app/config.yaml -v $(pwd)/data:/app/data llm_proxy:v2.0.0

# Stage 1: build the React dashboard (frontend/dist), served by the gateway
# as a static fallback (src/server/mod.rs -> ServeDir::new("frontend/dist")).
# Without this stage the admin UI returns 404 from a container image.
FROM node:22-slim AS frontend
WORKDIR /ui
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: build the Rust gateway
FROM rust:1.96-slim-bookworm AS builder
# deb.debian.org is unreachable from some networks; use a reachable mirror.
# Override with --build-arg APT_MIRROR=... if a different one works for you.
ARG APT_MIRROR=https://mirrors.tuna.tsinghua.edu.cn/debian
ARG APT_MIRROR_SECURITY=https://mirrors.tuna.tsinghua.edu.cn/debian-security
RUN printf 'Types: deb\nURIs: %s\nSuites: bookworm bookworm-updates\nComponents: main\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\n\nTypes: deb\nURIs: %s\nSuites: bookworm-security\nComponents: main\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\n' "$APT_MIRROR" "$APT_MIRROR_SECURITY" > /etc/apt/sources.list.d/debian.sources \
  && apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev libsqlite3-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY migrations/ migrations/
COPY src/ src/
COPY config.example.yaml ./
RUN cargo build --release --features cli && strip target/release/llm_proxy

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/llm_proxy /usr/local/bin/llm_proxy
COPY --from=frontend /ui/dist /app/frontend/dist
COPY --from=builder /build/migrations/ migrations/
RUN mkdir -p /app/data
VOLUME ["/app/data"]
EXPOSE 8080
ENV RUST_LOG=info
ENV LLM_PROXY_CONFIG=/app/config.yaml
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD /usr/local/bin/llm_proxy health-check || exit 1
ENTRYPOINT ["/usr/local/bin/llm_proxy"]
