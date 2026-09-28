# syntax=docker/dockerfile:1

FROM node:22-bookworm-slim AS web-build
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.96-bookworm AS rust-build
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY server/Cargo.toml server/Cargo.toml
COPY src-tauri/Cargo.toml src-tauri/Cargo.toml
COPY xtask/Cargo.toml xtask/Cargo.toml
COPY src/ src/
COPY server/ server/
COPY src-tauri/ src-tauri/
COPY xtask/ xtask/
COPY --from=web-build /app/web/dist web/dist
RUN cargo build --release -p raydium-debugger-server --locked

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust-build /app/target/release/raydium-debugger-server /usr/local/bin/raydium-debugger-server
COPY --from=web-build /app/web/dist web/dist
RUN useradd --system --user-group --home-dir /app raydium \
    && mkdir -p /data \
    && chown -R raydium:raydium /app /data
USER raydium
ENV RAYDIUM_DEBUGGER_CASEBOOK_PATH=/data/casebooks.json
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD curl -fsS http://127.0.0.1:8787/api/health || exit 1
CMD ["raydium-debugger-server", "--bind", "0.0.0.0:8787", "--allow-non-loopback"]
