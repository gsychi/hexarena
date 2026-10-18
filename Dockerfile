# Single image: the Axum game server also serves the built web app.

FROM node:22-slim AS web
WORKDIR /web
RUN corepack enable && corepack prepare pnpm@10.33.4 --activate
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

FROM rust:1-slim AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY sdk sdk
RUN cargo build --release -p hexarena-server

FROM debian:bookworm-slim
RUN useradd --system --create-home hexarena && mkdir -p /data && chown hexarena /data
COPY --from=server /src/target/release/hexarena-server /usr/local/bin/hexarena-server
COPY --from=web /web/dist /app/web
USER hexarena
ENV HEXARENA_HOST=0.0.0.0 \
    HEXARENA_PORT=8080 \
    HEXARENA_WEB=/app/web \
    HEXARENA_DATA=/data
EXPOSE 8080
VOLUME /data
CMD ["hexarena-server"]
