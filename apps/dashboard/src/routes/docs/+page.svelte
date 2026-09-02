<script lang="ts">
	import { session } from '$lib/session.svelte';

	const openai = $derived(`import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "${session.gatewayUrl}/v1",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});`);

	const anthropic = $derived(`import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "${session.gatewayUrl}",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});`);

	const deepseek = $derived(`import OpenAI from "openai";

// DeepSeek SDK / OpenAI SDK with DeepSeek base-URL convention
const client = new OpenAI({
  baseURL: "${session.gatewayUrl}",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

await client.chat.completions.create({
  model: "deepseek-chat",
  messages: [{ role: "user", content: "hello" }],
});`);

	const glm = $derived(`import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "${session.gatewayUrl}/v4",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

await client.chat.completions.create({
  model: "glm-4",
  messages: [{ role: "user", content: "hello" }],
  // glm-4.5 extras are forwarded:
  // thinking: { type: "enabled" }, do_sample: true, request_id: "..."
});`);

	const cursor = $derived(`Cursor Settings → Models
1. Toggle OpenAI API Key / override
2. OpenAI Base URL: ${session.gatewayUrl}/v1
3. OpenAI API Key: a ClosedRouter sk-cr-… key
4. Model: a friendly ID from the Models page

Cursor calls GET /v1/models and POST /v1/chat/completions (SSE).`);

	const langchain = $derived(`from langchain_openai import ChatOpenAI

llm = ChatOpenAI(
    base_url="${session.gatewayUrl}/v1",
    api_key="sk-cr-...",
    model="llama3",
)`);

	const hermes = $derived(`POST ${session.gatewayUrl}/v1/hermes/memories
Authorization: Bearer sk-cr-...
{
  "content": "user prefers terse answers",
  "agent": "hermes",
  "embed": true
}

POST ${session.gatewayUrl}/v1/hermes/memories/search
{ "query": "preferences", "embed": true, "limit": 8 }`);

	async function copy(text: string): Promise<void> {
		await navigator.clipboard.writeText(text);
	}
</script>

<div class="mx-auto max-w-5xl">
	<p class="text-xs tracking-[0.28em] text-mist uppercase">Connect</p>
	<h1 class="mt-2 text-3xl font-semibold tracking-tight">Client setup</h1>
	<p class="mt-2 text-sm text-mist">
		ClosedRouter speaks OpenAI, Anthropic, DeepSeek, GLM, and Cursor. Auth is a Bearer token issued
		on the API keys page.
	</p>

	<section class="mt-8">
		<div class="flex items-center justify-between gap-3">
			<h2 class="text-lg font-medium">Cursor</h2>
			<button type="button" class="text-sm text-acid" onclick={() => copy(cursor)}>Copy</button>
		</div>
		<pre
			class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{cursor}</pre>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">OpenAI SDK</h2>
		<pre
			class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{openai}</pre>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">Anthropic SDK</h2>
		<pre
			class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{anthropic}</pre>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">DeepSeek</h2>
		<pre
			class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{deepseek}</pre>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">GLM / Zhipu</h2>
		<pre
			class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{glm}</pre>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">LangChain</h2>
		<pre
			class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{langchain}</pre>
		<p class="mt-3 text-sm text-mist">
			Python and TypeScript samples live in <span class="font-mono">examples/langchain</span>.
		</p>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">Hermes memory</h2>
		<pre
			class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{hermes}</pre>
		<p class="mt-3 text-sm text-mist">
			Embeddings are 1536-d via an OpenAI-compat catalog model with capability=embedding. See
			docs/HERMES.md.
		</p>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">Langfuse</h2>
		<p class="mt-2 text-sm text-mist">
			<code class="font-mono text-acid">docker compose --profile full up</code> starts Langfuse on
			port 3001. Set <span class="font-mono">LANGFUSE_ENABLED=true</span> plus public/secret keys to export
			generations (model, tokens, latency, key id).
		</p>
	</section>
</div>
