FROM oven/bun:1 AS frontend
WORKDIR /app
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY web/ ./
ENV VITE_API_URL=
RUN bun run build

FROM rust:1-slim AS backend
RUN apt-get update && apt-get install -y build-essential && apt-get install -y curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY api/Cargo.toml api/Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src
COPY api/src/ ./src/
COPY api/migrations/ ./migrations/
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=caddy:2-alpine /usr/bin/caddy /usr/local/bin/caddy
COPY --from=backend /app/target/release/api /usr/local/bin/api
COPY --from=frontend /app/build /srv/frontend
COPY Caddyfile /etc/caddy/Caddyfile
COPY docker-entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
ENV DATABASE_URL=sqlite:/data/data.db?mode=rwc
ENV HOST=0.0.0.0
ENV PORT=3001
ENV XDG_DATA_HOME=/data
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 CMD curl -f http://localhost:3001/api/health || exit 1
ENTRYPOINT ["/entrypoint.sh"]
