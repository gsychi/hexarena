# Deploying Hexarena

Production is one container. `hexarena-server` serves the API under `/api` and the built web app from `HEXARENA_WEB`, and it stores games as JSON files in `HEXARENA_DATA`, which must be a persistent volume.

| Variable | Default | Purpose |
| --- | --- | --- |
| `HEXARENA_HOST` | `127.0.0.1` (`0.0.0.0` in the image) | Bind address |
| `HEXARENA_PORT` / `PORT` | `47240` (`8080` in the image) | Listen port |
| `HEXARENA_WEB` | unset (`/app/web` in the image) | Built web app to serve; unset means API only |
| `HEXARENA_DATA` | `data/matches` (`/data` in the image) | Game storage |

Health check: `GET /api/health` returns `ok`.

## Any Docker host

```bash
docker build -t hexarena .
docker run -p 8080:8080 -v hexarena-data:/data hexarena
```

## Fly.io

```bash
fly launch --copy-config --no-deploy      # uses fly.toml in the repo root; pick an app name
fly volumes create hexarena_data --size 1
fly deploy
```

## Without Docker

```bash
cd web && pnpm install && pnpm build && cd ..
cargo build --release -p hexarena-server
HEXARENA_HOST=0.0.0.0 HEXARENA_PORT=8080 HEXARENA_WEB=web/dist HEXARENA_DATA=/var/lib/hexarena ./target/release/hexarena-server
```

Put it behind a TLS-terminating proxy. Game creation is unauthenticated. Each request may create at most 20 games, and the server refuses new games while 100 are already queued or running.
