# Architecture

ClosedRouter is a self-hosted LLM API gateway: clients speak OpenAI, Anthropic, DeepSeek, GLM, or Cursor; the gateway authenticates ClosedRouter keys, remaps friendly model IDs onto upstream providers, translates protocols when needed, and records bounded request logs.

```
Cursor / OpenAI SDK / Anthropic SDK / DeepSeek SDK / GLM SDK / LangChain
        │
        ▼
   Traefik (compose profile `full`, optional)
        │
        ▼
   Rust gateway (Axum)  :8080
        │  Postgres + pgvector
        │  optional Langfuse export
        ▼
   Upstream: Ollama, vLLM, OpenAI, Anthropic, DeepSeek, GLM, …
```

## Processes

| Process | Role |
| --- | --- |
| `closedrouter` (`crates/gateway`) | Axum HTTP API, translation, Hermes memory, `/metrics` |
| `apps/dashboard` | SvelteKit 5 admin (adapter-node) |
| `apps/landing` | Astro marketing site (adapter-vercel) |
| Postgres 16 + pgvector | Keys, catalog, request logs, Hermes embeddings |
| Traefik / Grafana / Loki / Prometheus / Langfuse | Observability; compose profile `full` |

## Request path

1. Client sends `Authorization: Bearer sk-cr-…` (or `x-api-key`) to `/v1/chat/completions`, `/v1/messages`, `/chat/completions` (DeepSeek), or `/v4/chat/completions` (GLM).
2. The gateway hashes the key and looks it up in `api_keys`.
3. `model` is resolved via `models` → `providers`.
4. If the incoming protocol family matches the upstream (`openai` / `deepseek` / `glm` are one family; `anthropic` is the other), only `model` is rewritten so extra fields survive (`reasoning_content`, `tools`, GLM `thinking` / `do_sample` / `request_id`, `stream_options`).
5. Cross-family requests are translated, including SSE.
6. Prometheus counters/histograms update; a row is appended to `request_logs` (oldest rows dropped past `REQUEST_LOG_LIMIT`); optional Langfuse generation-create is fired in the background.

## Modules (`crates/gateway/src`)

- `lib.rs` — router, CORS (any headers for Cursor), boot
- `config.rs` — YAML + env
- `db.rs` — sqlx Postgres + pgvector
- `translate.rs` — OpenAI ↔ Anthropic ↔ DeepSeek ↔ GLM
- `upstream.rs` — reqwest proxy + embeddings
- `hermes.rs` — memory HTTP API
- `observability.rs` — Prometheus + Langfuse
- `auth.rs` / `routes.rs` / `error.rs`

## Data

See `crates/gateway/src/schema.sql`. Hermes embeddings are **1536-d** (`vector(1536)`), matching OpenAI `text-embedding-3-small`.
