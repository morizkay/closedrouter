# ClosedRouter

OpenRouter, but local. A self-hosted LLM API gateway with **OpenAI-compatible** and **Anthropic-compatible** APIs, a SvelteKit dashboard, and a Vercel-ready marketing site.

Your models, your keys, your network.

## Quick start (Docker)

```bash
cp .env.example .env
docker compose up --build
```

- Gateway: [http://localhost:8080](http://localhost:8080) (`GET /health`)
- Dashboard: [http://localhost:3000](http://localhost:3000)

Sign in to the dashboard with `ADMIN_TOKEN` from `.env`. Create an API key, add a provider (Ollama, vLLM, Groq, OpenAI, Anthropic, …), and map a friendly model ID.

First boot can seed Ollama from `config.example.yaml` (copied into the image as `/etc/closedrouter/config.yaml`). Override with `CLOSEDROUTER_CONFIG` or the dashboard.

## Without Docker

**Gateway** (Rust 1.88+):

```bash
cp .env.example .env
cargo run -p closedrouter-gateway
```

Binds `HOST:PORT` (default `0.0.0.0:8080`). SQLite lives at `DATABASE_PATH`.

**Dashboard**:

```bash
cd apps/dashboard
npm install
PUBLIC_GATEWAY_URL=http://localhost:8080 npm run dev
```

**Landing** (this is the Vercel site):

```bash
cd apps/landing
npm install
npm run dev
```

## Client examples

OpenAI SDK:

```ts
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:8080/v1",
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

Cursor: set the OpenAI base URL to `http://localhost:8080/v1` and the API key to a ClosedRouter key.

## Layout

| Path | Role |
| --- | --- |
| `crates/gateway` | Axum gateway: `/v1/chat/completions`, `/v1/messages`, `/v1/models`, `/health`, `/admin/v1/*` |
| `apps/dashboard` | Admin UI (keys, providers, models, playground, docs) |
| `apps/landing` | Marketing site — **Vercel project root** |
| `docker-compose.yml` | Gateway 8080 + dashboard 3000 |

## Vercel (landing only)

The Rust gateway is **not** deployed on Vercel. Deploy `apps/landing`:

1. New Vercel project from this repo
2. Set **Root Directory** to `apps/landing`
3. Framework: SvelteKit (`@sveltejs/adapter-vercel` is already configured)
4. Build command: `npm run build`

`apps/landing/vercel.json` marks the framework. Do not set the repo root as the Vercel project unless you also change Root Directory.

## Config

See `.env.example` and `config.example.yaml`. Important variables:

- `HOST` / `PORT` — gateway bind (`0.0.0.0` for Docker/cloud)
- `DATABASE_PATH` — SQLite file
- `ADMIN_TOKEN` — dashboard secret
- `CORS_ORIGINS` — comma-separated, or `*`
- `PUBLIC_GATEWAY_URL` — URL the **browser** uses to reach the gateway

## License

MIT
