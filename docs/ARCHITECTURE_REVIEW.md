# ClosedRouter Architecture Review

**Reviewed branch:** `cursor/closedrouter-product-6e01` (product tree; `main` is still the initial commit only)  
**Reviewer:** Cursor Cloud Agent  
**Date:** 2026-09-01  
**Scope:** ~25k LOC across 99 files added on the product branch — gateway, dashboard, landing, Docker/observability, Hermes, examples, CI.

This review is based on direct code inspection and running `cargo test --workspace` locally. It is not a marketing summary of the prior agent’s work.

---

## Executive summary

ClosedRouter is a **single-crate Axum gateway** with a **Postgres catalog**, **protocol translation layer**, **optional Hermes memory**, and **two frontends** (React admin dashboard, Astro marketing site). The core idea is sound and the translation code is more serious than typical “thin proxy” projects: there are unit tests for tool calls, reasoning fields, SSE framing, and catalog seeding edge cases.

The main risks are not “missing features” but **operational security** (SSRF via provider URLs, plaintext upstream keys, open metrics), **protocol completeness** (cross-family streaming is complex and under-tested at the HTTP boundary), and **schema/seed semantics** that will confuse operators as the product matures. The observability stack is appropriately gated behind `docker compose --profile full`, but several defaults in that profile are unsafe for anything beyond local dev.

---

## Current architecture (honest)

### Repository shape

| Path | What it actually is |
| --- | --- |
| `crates/gateway` | **The product.** One library + `closedrouter` binary. All routing, auth, translation, upstream proxy, DB, Hermes, metrics, Langfuse export. |
| `apps/dashboard` | React 19 + Vite admin UI with TanStack Router/Query and a static Node server. **Browser-only:** talks to gateway admin API with `ADMIN_TOKEN` from `localStorage`. |
| `apps/landing` | Astro static marketing site (`@astrojs/vercel`). **Fully decoupled** from gateway runtime. |
| `docker-compose.yml` | Slim default: Postgres + gateway + dashboard. `full` profile adds Traefik, Prometheus, Loki, Promtail, Grafana, Langfuse. |
| `examples/langchain` | Manual scripts, not CI-gated. |
| `deploy/*` | Prometheus/Loki/Grafana/Promtail configs and a one-line Postgres init (`CREATE DATABASE langfuse`). |

There is **no multi-crate Rust workspace boundary** beyond the gateway. Module names (`translate`, `upstream`, `db`, `auth`, `routes`, `hermes`) describe concerns, but everything compiles into one artifact and shares `AppState`. That is fine for v0.1; it is not yet a layered architecture with enforced dependency direction.

### Request path (chat)

```
Client (OpenAI / Anthropic / DeepSeek / GLM / Cursor SDK)
  → Axum route (/v1/chat/completions, /v1/messages, /chat/completions, /v4/chat/completions)
  → ApiAuth (Bearer or x-api-key → SHA-256 lookup in api_keys)
  → db::resolve_model (friendly id → provider + upstream_model)
  → upstream::proxy
       same protocol family: rewrite_model only (preserves extra JSON fields)
       cross family: translate.rs request rewrite
       stream: pipe_stream with SSE translators OR byte passthrough
  → async side effects: request_logs insert, optional Langfuse generation-create
```

**Where things really live:**

- **Protocol translation:** `translate.rs` (request/response JSON + SSE event mapping). This is the shared kernel; DeepSeek and GLM are treated as OpenAI-family at the translation layer (`Protocol::is_openai_family`).
- **Routing / proxy:** `upstream.rs` (HTTP client, auth headers per provider kind, streaming pipe).
- **Auth:** `auth.rs` (extractors only). Admin and API keys are separate; no RBAC beyond that.
- **Persistence:** `db.rs` + embedded `schema.sql`. Applied on every connect via `apply_schema`, not versioned migrations.
- **HTTP surface:** `routes.rs` + `lib.rs::router`.

### Data

- Postgres 16 + pgvector (`vector(1536)` for Hermes).
- Tables: `settings`, `api_keys`, `providers`, `models`, `request_logs`, `hermes_sessions`, `hermes_memories`.
- Provider upstream API keys stored **in plaintext** in `providers.api_key`.
- ClosedRouter client keys stored as **SHA-256 hashes** (appropriate for high-entropy `sk-cr-…` tokens).
- `ADMIN_TOKEN` may come from env, else `settings` table, else auto-generated and persisted (with warnings).

### Dashboard and landing

- **Dashboard:** React 19 with TanStack Router/Query. No server-side BFF; CORS must allow the browser origin. Pages: overview, keys, models/providers CRUD (create/delete only in UI), playground (non-streaming chat), inline docs.
- **Landing:** Astro components, no shared code with dashboard. Correct separation for Vercel deploy.

### Tests and CI

`.github/workflows/ci.yml`:

- Rust: `clippy -D warnings`, `cargo test` with Postgres service + `DATABASE_URL`.
- Dashboard + landing: `npm ci`, TypeScript / Astro check, prettier/eslint.

**What tests actually prove:**

| Area | Coverage | Gap |
| --- | --- | --- |
| Translation JSON | Strong unit tests in `translate.rs` | Does not cover every OpenAI/Anthropic field |
| SSE helpers | Unit tests for framing, tool deltas, UTF-8 chunk splits | No HTTP-level streaming integration tests |
| Upstream proxy | wiremock tests for non-streaming paths | Streaming proxy untested end-to-end |
| DB / seed | Postgres tests for schema concurrency, Hermes, seed idempotency/rollback/resume | Skipped locally without Postgres/Docker |
| Auth | Two router tests (health, 401 without key) | No admin auth tests |
| Dashboard / landing | Typecheck + lint only | No Playwright/e2e |

Locally, with no Postgres: **37 tests pass** (translation + wiremock). With `DATABASE_URL` set but no server: **7 tests fail hard** (by design — they assert misconfiguration rather than skip).

---

## Protocol layer assessment

### Shared kernel vs copy-paste

**Good:** One `Protocol` enum, one `translate.rs`, family-aware passthrough (`rewrite_model` keeps unknown fields on same-family hops). DeepSeek `reasoning_content` and GLM `thinking` / `do_sample` / `request_id` are explicitly considered.

**Risky:** Cross-family translation only forwards a **whitelist** of fields (`temperature`, `top_p`, `stream`, `stop`→`stop_sequences`, `tools`, etc.). OpenAI params like `response_format`, `n`, `seed`, `logprobs`, `tool_choice`, `parallel_tool_calls`, `presence_penalty`, `frequency_penalty`, `json_schema`, and most Anthropic beta fields are **silently dropped** when translating. Same-family passthrough avoids this; cross-family does not.

### Streaming / SSE

- Same-family streams are **byte-passthrough** (correct for fidelity).
- Cross-family uses custom SSE parsers (`push_sse_bytes`, `anthropic_event_to_openai_sse`, `openai_data_to_anthropic_sse`).
- Tool-call streaming has dedicated tests (OpenAI→Anthropic tool_use blocks, Anthropic→OpenAI tool metadata).
- `finish_reason` / `stop_reason` mapping exists; unknown reasons (e.g. `content_filter`) collapse to `end_turn` / `stop` without surfacing to the client.

**Missing:** No test that runs a full `POST /v1/chat/completions` with `stream: true` through the Axum router against wiremock upstream returning SSE. The hardest bugs will be in `pipe_stream` + partial JSON lines, not in isolated converter functions.

### Finish-reason and usage

- Non-streaming cross-family responses translate usage fields.
- **Streaming responses do not populate token counts** in `ProxyMeta` (`prompt_tokens` / `completion_tokens` stay `None`), so request logs and Langfuse under-report for streamed traffic — likely the majority of production usage.

---

## Security assessment

| Topic | Finding | Severity |
| --- | --- | --- |
| `ADMIN_TOKEN` | Compose requires non-empty, non-`change-me-now`. Good. Persisted to DB if env unset. | OK with caveats |
| API keys | Hashed at rest. Prefix stored for UI. | OK |
| Provider keys | Plaintext in Postgres. Admin API returns `has_api_key` only — good — but DB backup = secret leak. | High for prod |
| SSRF | **No validation** on `providers.base_url`. Admin (or seeded config) can point gateway at `http://169.254.169.254`, `http://postgres:5432`, internal k8s services, etc. Gateway will forward requests with stored credentials. | **P0** |
| `/metrics` | Unauthenticated Prometheus scrape on same port as API. | P0 on public deploys |
| `/health` | Unauthenticated. Acceptable. | OK |
| CORS | Docker default `*`. Needed for Cursor; increases XSS impact if dashboard and gateway share an origin policy mistake. | P1 |
| Dashboard token storage | `ADMIN_TOKEN` in `localStorage`. Any XSS in dashboard = full admin. | P1 |
| Compose defaults | Postgres `closedrouter/closedrouter`, Grafana `admin/admin` + anonymous Viewer, Langfuse `NEXTAUTH_SECRET` / `SALT` placeholders. Postgres bound to `127.0.0.1:5432` in compose (good). | P1 for `full` profile |
| Upstream auth fallback | OpenAI-family without provider key uses `Bearer closedrouter`. Harmless for Ollama; confusing and wrong for real APIs. | P2 |
| Rate limiting | None. | P1 |
| TLS | Not terminated in gateway; assumed external. Undocumented for production. | P2 |

---

## Data and migrations

### Schema strategy

`schema.sql` is `CREATE TABLE IF NOT EXISTS` + indexes. `apply_schema` runs on every boot. There is **no migration version table**. Adding a column requires careful idempotent SQL or manual ops — workable for early stage, brittle for teams.

`ensure_vector_extension` uses a Postgres advisory lock to avoid `CREATE EXTENSION` races. This is a thoughtful detail with tests.

### Seed behavior

**Actual behavior** (`db::seed_catalog`): inserts providers by **name** if missing; inserts models by **id** if missing; transactional per call. Survives partial first boot (tested). Does **not** update `base_url`, `api_key`, or `upstream_model` when config changes later.

**Documented behavior** (`config.example.yaml`, comments): “seeded only when the catalog is empty (first boot).” **This is wrong** and will mislead operators.

### Hermes / pgvector

- Fixed 1536-dim vectors, cosine distance via `<=>`.
- **No IVFFlat/HNSW index** on `hermes_memories.embedding`. Fine for hundreds of rows; will scan at scale.
- Text fallback uses `ILIKE '%query%'` (injection-safe via binding, but not full-text search).
- Embeddings never returned in API responses (good).

### Request log retention

`insert_request_log` appends then deletes oldest rows in batches when count > `REQUEST_LOG_LIMIT`. Deletes are not in the same transaction as insert; concurrent requests can overshoot briefly. Acceptable for v0.1.

---

## Overbuild vs missing

### Appropriate / keep (but gate)

| Component | Verdict |
| --- | --- |
| Traefik, Prometheus, Grafana, Loki, Promtail | Reasonable as **`full` profile** for self-hosters who want a demo ops stack. Not in the critical path. |
| Langfuse | Optional, fire-and-forget, minimal payload. Low maintenance cost. |
| LangChain examples | Fine as docs; don’t expand until core gateway is hardened. |

### Overbuilt relative to core maturity

- Grafana anonymous Viewer + default admin password in compose `full` profile is dangerous if someone exposes port 3002.
- Langfuse shares Postgres with gateway data; separate DB would isolate blast radius (P2).

### Missing relative to “OpenRouter-style gateway” expectations

- Per-key rate limits / quotas / budgets
- Request ID propagation and structured audit trail
- Provider URL allowlisting or SSRF controls
- Encrypted secret storage
- Streaming usage accounting
- Capability enforcement (`chat` vs `embedding` on resolve path)
- Dashboard edit flows (PATCH APIs exist; UI doesn’t use them)
- HTTP integration test suite for streaming translation
- Versioned migrations

---

## Ranked issues

### P0 — Correctness / security (fix before calling it production-ready)

1. **SSRF via provider `base_url`** — Admin-configured upstream URLs can target internal networks; gateway proxies with stored secrets.
2. **Plaintext upstream API keys in Postgres** — Backup, replica, or SQL injection exposes provider credentials.
3. **Unauthenticated `/metrics`** — Exposes operational data on the public API port; compose Prometheus scrapes gateway directly.
4. **Cross-protocol streaming untested at HTTP boundary** — High risk of silent breakage for Cursor/Anthropic/OpenAI combinations under load.
5. **Streaming requests omit token usage in logs/Langfuse** — Operational blind spot; billing/analytics wrong.

### P1 — Design debt that will hurt soon

1. **No schema migrations** — `schema.sql` only; evolving tables will be painful.
2. **Seed/docs mismatch + non-updating seed** — Operators will think YAML changes apply; they won’t (except new model ids).
3. **Monolithic gateway module graph** — `routes.rs` is a god-module; translation and persistence will keep colliding as features grow.
4. **No rate limiting or per-key quotas** — Any leaked `sk-cr-…` key can burn upstream spend.
5. **Hermes vector search without ANN index** — Latency grows linearly with memories per key.
6. **Admin token in `localStorage`** — Acceptable for local admin; weak for shared/multi-user dashboards.
7. **`DATABASE_URL` set but unreachable fails tests** — Correct for CI; document that local dev should unset `DATABASE_URL` or run Postgres.
8. **Translation whitelist** — Cross-family requests drop many standard API fields without warning.
9. **No `capability` check on chat resolve** — Embedding models can be invoked via chat endpoints.

### P2 — Cleanup / polish

1. Outdated comments (“catalog empty”, `config.rs` seed docs).
2. Grafana/Langfuse default secrets in `full` profile.
3. Playground doesn’t exercise streaming or Anthropic routes.
4. `examples/langchain` not in CI.
5. `testcontainers` container `mem::forget` leaks Docker container for process lifetime.
6. `bearer_auth("closedrouter")` placeholder when provider key missing.
7. Landing and dashboard share no design system (fine for now).

---

## Highest-leverage next changes (3–5)

### 1. SSRF guardrails on provider URLs

**Why:** This is the clearest path from “self-hosted admin” to “internal network proxy.” Block link-local, metadata IPs, private ranges (configurable), and non-http(s) schemes at provider create/update. Add tests.

**Not a rewrite:** A validator in `db::create_provider` / `update_provider` plus optional `ALLOW_PRIVATE_UPSTREAMS=true` for lab Ollama.

### 2. HTTP integration tests for streaming translation

**Why:** Unit tests in `translate.rs`/`upstream.rs` won’t catch Axum + hyper + chunked SSE bugs. Add wiremock upstream that emits Anthropic SSE; assert OpenAI client SSE shape from `POST /v1/chat/completions` with `stream: true` (and the reverse).

**Not a rewrite:** One or two tests in `lib.rs` or a `tests/` directory using the existing `router()` + wiremock pattern.

### 3. Versioned SQL migrations (sqlx migrate or equivalent)

**Why:** Hermes, catalog, and logs will need schema changes. `CREATE IF NOT EXISTS` does not scale for ALTERs, backfills, or index additions (e.g. pgvector HNSW).

**Not a rewrite:** Introduce `migrations/` folder; keep `apply_schema` as bootstrap for empty DB once.

### 4. Protect or split `/metrics`

**Why:** Prometheus needs scrape access; the world does not. Options: separate bind address, Bearer token, or network policy documented in compose (metrics only on internal Docker network, drop host port publish).

**Not a rewrite:** Env flag `METRICS_BIND` or auth middleware on `/metrics` only.

### 5. Encrypt provider secrets at rest (or document explicit threat model)

**Why:** Plaintext `api_key` in Postgres is the main data-at-rest gap. Minimum: envelope encryption with `MASTER_KEY` env; better: integrate with Docker/K8s secrets and don’t persist upstream keys (require env injection per provider).

**Not a rewrite:** Start with encryption helper around `providers.api_key` read/write.

---

## What to explicitly NOT touch yet

- **Do not split into microservices** — The single binary is appropriate until team size or scale demands otherwise.
- **Do not replace Postgres or remove pgvector** — Fits Hermes and catalog well.
- **Do not rip out the `full` observability profile** — It is already optional; trim defaults instead of deleting the stack.
- **Do not rebuild the dashboard as a server-side admin proxy** — Larger change; SSRF and secret storage matter more than BFF pattern right now.
- **Do not chase full OpenAPI field parity** — Prioritize the cross-family paths and fields your actual clients use (Cursor OpenAI, Anthropic Messages, tool streaming).
- **Do not merge `cursor/closedrouter-product-6e01` to `main` without addressing P0 SSRF and metrics exposure** — `main` is empty; the product branch is the real codebase, but it is not production-hardened as-is.

---

## Appendix: module boundary reality check

```
crates/gateway/src/
  lib.rs      → boot, router, admin token resolution, CORS
  routes.rs   → all HTTP handlers (chat, admin, metrics, health)
  auth.rs     → ApiAuth, AdminAuth extractors
  translate.rs→ protocol conversion (largest pure-logic module)
  upstream.rs → HTTP proxy + SSE pipe
  db.rs       → all SQL + seed + Hermes queries
  hermes.rs   → thin handlers over db + embed_via_catalog
  config.rs   → YAML + env
  observability.rs → Prometheus + Langfuse
  error.rs    → OpenAI-shaped errors
```

**Verdict:** Boundaries are **organizational**, not enforced. `routes` knows about Hermes, translation, upstream, and DB. Extraction into crates (`closedrouter-translate`, `closedrouter-store`) would help only after the protocol and security baselines stabilize.

---

## Appendix: CI and local test commands

```bash
# Matches CI Rust job
DATABASE_URL=postgres://closedrouter:closedrouter@localhost:5432/closedrouter \
  cargo clippy -p closedrouter-gateway --all-targets -- -D warnings
DATABASE_URL=... cargo test --workspace

# UI
cd apps/dashboard && npm ci && npm run check && npm run lint
cd apps/landing && npm ci && npm run check && npm run lint
```

Without Postgres: expect **37 passing** unit/wiremock tests; DB-dependent tests skip only when `DATABASE_URL` is unset **and** testcontainers cannot start.
