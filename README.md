# ClosedRouter

OpenRouter, but local. A self-hosted LLM API gateway with **OpenAI**, **Anthropic**, **DeepSeek**, **GLM**, and **Cursor**-compatible APIs, Hermes memory (pgvector), a SvelteKit dashboard, and a Vercel-ready marketing site.

Your models, your keys, your network.

## Quick start (Docker)

Slim stack — Postgres, gateway, dashboard:

```bash
cp .env.example .env
docker compose up --build
```

- Gateway: [http://localhost:8080](http://localhost:8080) (`GET /health`)
- Dashboard: [http://localhost:3000](http://localhost:3000)
- Postgres: `localhost:5432` (pgvector)

Full observability stack (Traefik, Prometheus, Loki, Promtail, Grafana, Langfuse):

```bash
docker compose --profile full up --build
```

| Service | Slim | Full |
| --- | --- | --- |
| Gateway | :8080 | :8080 and `http://gateway.localhost` |
| Dashboard | :3000 | :3000 and `http://dashboard.localhost` |
| Grafana | — | :3002 / `http://grafana.localhost` |
| Prometheus | — | :9090 |
| Loki | — | :3100 |
| Langfuse | — | :3001 / `http://langfuse.localhost` |
| Traefik | — | :80 (dashboard :8081) |

Sign in to the dashboard with `ADMIN_TOKEN` from `.env`. Create an API key, add a provider, and map a friendly model ID.

## Without Docker

Run Postgres 16 with pgvector, then:

```bash
cp .env.example .env
# DATABASE_URL=postgres://closedrouter:closedrouter@localhost:5432/closedrouter
cargo run -p closedrouter-gateway
```

Binds `HOST:PORT` (default `0.0.0.0:8080`).

**Dashboard**:

```bash
cd apps/dashboard
npm install
PUBLIC_GATEWAY_URL=http://localhost:8080 npm run dev
```

**Landing** (Vercel site):

```bash
cd apps/landing
npm install
npm run dev
```

## Cursor

1. Cursor Settings → **Models**
2. Enable **OpenAI API Key** / override
3. **OpenAI Base URL**: `http://localhost:8080/v1` (or `http://gateway.localhost/v1` with Traefik)
4. **OpenAI API Key**: a ClosedRouter `sk-cr-…` key
5. Set the model override to a catalog id (for example `llama3`)

Cursor uses `/v1/chat/completions` and `/v1/models`. Streaming is standard OpenAI SSE (`data: …` / `data: [DONE]`). CORS allows Cursor’s extra headers (`http-referer`, `x-title`, `OpenAI-Beta`, …).

## Client examples

OpenAI / DeepSeek (OpenAI SDK):

```ts
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:8080/v1",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});
```

DeepSeek SDK convention (base URL without `/v1`):

```ts
const client = new OpenAI({
  baseURL: "http://localhost:8080",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});
```

Anthropic SDK (`baseURL` is the gateway origin, not `/v1`):

```ts
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "http://localhost:8080",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});
```

GLM / Zhipu OpenAI-compat: `baseURL: "http://localhost:8080/v4"` or `/v1`.

LangChain: see [`examples/langchain`](examples/langchain).  
Hermes memory: see [`docs/HERMES.md`](docs/HERMES.md).  
API surface: [`docs/API.md`](docs/API.md).  
Internals: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Langfuse

Compose `full` runs Langfuse 2 at `:3001`. Create a project, copy public/secret keys, then:

```bash
LANGFUSE_ENABLED=true
LANGFUSE_HOST=http://localhost:3001
LANGFUSE_PUBLIC_KEY=pk-lf-...
LANGFUSE_SECRET_KEY=sk-lf-...
```

The gateway POSTs `generation-create` events (model, tokens, latency, key id) to `/api/public/ingestion`. Disable anytime with `LANGFUSE_ENABLED=false`.

## Tests

```bash
make test
# or
cargo test --workspace
cd apps/dashboard && npm run check
```

Translation unit tests and wiremock upstream tests do **not** need API keys. Postgres tests run when `DATABASE_URL` is set, or via testcontainers (`pgvector/pgvector:pg16`) if Docker is available; otherwise they skip.

```bash
cargo clippy -p closedrouter-gateway --all-targets -- -D warnings
```

## Layout

| Path | Role |
| --- | --- |
| `crates/gateway` | Axum gateway |
| `apps/dashboard` | Admin UI |
| `apps/landing` | Marketing site — **Vercel project root** |
| `docker-compose.yml` | Slim default; `--profile full` for observability |
| `examples/langchain` | ChatOpenAI / ChatAnthropic |
| `docs/` | Architecture, API, Hermes |

## Vercel (landing only)

The Rust gateway is **not** deployed on Vercel. Deploy `apps/landing` with **Root Directory** `apps/landing`.

## Config

See `.env.example` and `config.example.yaml`.

- `HOST` / `PORT` — gateway bind (`0.0.0.0` for Docker/cloud)
- `DATABASE_URL` — Postgres (required)
- `ADMIN_TOKEN` — dashboard secret
- `CORS_ORIGINS` — comma-separated, or `*`
- `EMBEDDING_MODEL` — catalog id for Hermes `embed: true`
- `LANGFUSE_*` — optional trace export
- `PUBLIC_GATEWAY_URL` — URL the **browser** uses to reach the gateway

## License

MIT
