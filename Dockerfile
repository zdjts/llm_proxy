# llm_proxy v2.0.0 — Dockerfile (multi-stage, minimal)
# Build:  docker build -t llm_proxy:v2.0.0 .
# Run:    docker run -p 8080:8080 -v $(pwd)/config.yaml:/app/config.yaml -v $(pwd)/data:/app/data llm_proxy:v2.0.0

FROM rust:1.96-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
COPY RUNBOOK.md ./
COPY migrations/ migrations/
COPY src/ src/
COPY config.example.yaml ./
RUN cargo build --release --features cli && strip target/release/llm_proxy

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/llm_proxy /usr/local/bin/llm_proxy
COPY --from=builder /build/migrations/ migrations/
COPY --from=builder /build/RUNBOOK.md ./
RUN mkdir -p /app/data
VOLUME ["/app/data"]
EXPOSE 8080
ENV RUST_LOG=info
ENV LLM_PROXY_CONFIG=/app/config.yaml
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD /usr/local/bin/llm_proxy health-check || exit 1
ENTRYPOINT ["/usr/local/bin/llm_proxy"]
