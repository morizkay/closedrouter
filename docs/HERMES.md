# Hermes memory contract

Hermes (or any agent) stores and recalls notes scoped to a ClosedRouter API key. Vectors live in Postgres `pgvector` with a fixed **1536** dimensions (OpenAI `text-embedding-3-small` / compatible `/v1/embeddings`).

Auth: the same `sk-cr-…` Bearer key used for chat. Memories are isolated per `key_id`.

## Sessions (threads)

```http
POST /v1/hermes/sessions
{"title": "optional thread title"}
```

```http
GET /v1/hermes/sessions
```

Response item:

```json
{
  "id": "uuid",
  "key_id": "uuid",
  "title": "optional thread title",
  "created_at": "RFC3339"
}
```

## Store

```http
POST /v1/hermes/memories
{
  "content": "user prefers terse answers",
  "session_id": "optional-session-uuid",
  "agent": "hermes",
  "metadata": { "source": "chat" },
  "embedding": [0.0],
  "embed": true
}
```

- `content` (required)
- `session_id` must belong to this key if set
- `embedding` optional, must be length 1536
- `embed: true` calls the catalog embedding model (`EMBEDDING_MODEL` or the first model with `capability=embedding`) via OpenAI-compat `/embeddings` on that provider
- If neither `embedding` nor `embed` is set, the row is stored without a vector (text search still works)

## Search / recall

```http
POST /v1/hermes/memories/search
{
  "query": "what does the user prefer?",
  "embedding": null,
  "session_id": null,
  "agent": "hermes",
  "limit": 8,
  "embed": true
}
```

- Vector search if `embedding` is provided, or if `embed: true` with `query` (query is embedded first)
- Otherwise `ILIKE` on `content`
- Hits include `distance` (cosine distance from pgvector `<=>`) when vector search ran

## List by session

```http
GET /v1/hermes/memories?session_id=<uuid>&limit=50
```

Omitting `session_id` lists recent memories for the key (newest first). Embeddings are never returned in list/search payloads.

## Schema

```
hermes_sessions(id, key_id, title, created_at)
hermes_memories(id, key_id, session_id, agent, content, embedding vector(1536), metadata jsonb, created_at)
```
