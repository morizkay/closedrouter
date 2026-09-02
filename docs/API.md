# API reference

Base URL is the gateway origin (default `http://localhost:8080`). ClosedRouter keys (`sk-cr-…`) go in `Authorization: Bearer` or `x-api-key`. Admin calls use `ADMIN_TOKEN` the same way, or `x-admin-token`. Compose requires `ADMIN_TOKEN` in `.env`; the gateway refuses empty values and the example placeholder `change-me-now`.

## Health and metrics

| Method | Path | Auth |
| --- | --- | --- |
| GET | `/health` | none |
| GET | `/metrics` | none (Prometheus text) |

## OpenAI / Cursor

| Method | Path |
| --- | --- |
| GET | `/v1/models` |
| POST | `/v1/chat/completions` |
| POST | `/v1/embeddings` |

`GET /v1/models` returns Cursor-friendly objects (`permission`, `root`, `parent`). Streaming is SSE `data: {json}\n\n` terminated with `data: [DONE]`. `stream_options.include_usage` is forwarded. Tool / function calls pass through on the OpenAI family.

## Anthropic

| Method | Path |
| --- | --- |
| POST | `/v1/messages` |

`anthropic-version` is forwarded to Anthropic-kind upstreams as `2023-06-01` when ClosedRouter originates the call.

## DeepSeek

Same Chat Completions body as OpenAI. Extra field `reasoning_content` (request messages and responses) is preserved on same-family hops and mapped to Anthropic thinking blocks when translating.

| Method | Path |
| --- | --- |
| POST | `/v1/chat/completions` |
| POST | `/chat/completions` |

Point the DeepSeek SDK or `OpenAI` with `baseURL: http://localhost:8080` (SDK already appends `/chat/completions`) **or** `http://localhost:8080/v1`.

## GLM (Zhipu / ChatGLM)

OpenAI-compat plus GLM fields `thinking`, `do_sample`, `request_id`. Aimed at glm-4 / glm-4.5 self-host and cloud OpenAI-compat.

| Method | Path |
| --- | --- |
| POST | `/v1/chat/completions` |
| POST | `/v4/chat/completions` |

Set the GLM / Zhipu client base URL to `http://localhost:8080/v4` or `http://localhost:8080/v1`. Provider `kind` is `glm` (aliases `zhipu`, `chatglm`). Typical cloud `base_url`: `https://open.bigmodel.cn/api/paas/v4`.

## Hermes memory

Documented in [HERMES.md](./HERMES.md).

## Admin

| Method | Path |
| --- | --- |
| GET | `/admin/v1/status` |
| GET/POST | `/admin/v1/keys` |
| DELETE | `/admin/v1/keys/{id}` |
| GET/POST | `/admin/v1/providers` |
| PATCH/DELETE | `/admin/v1/providers/{id}` |
| GET/POST | `/admin/v1/models` |
| PATCH/DELETE | `/admin/v1/models/{id}` |

Provider `kind`: `openai` \| `anthropic` \| `deepseek` \| `glm`.  
Model `capability`: `chat` (default) \| `embedding`.
