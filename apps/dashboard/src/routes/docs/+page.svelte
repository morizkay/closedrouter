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

	const cursor = $derived(`OpenAI Base URL: ${session.gatewayUrl}/v1
OpenAI API Key: (a ClosedRouter key)
Model: a friendly ID from the Models page`);
</script>

<div class="mx-auto max-w-5xl">
	<p class="text-xs tracking-[0.28em] text-mist uppercase">Connect</p>
	<h1 class="mt-2 text-3xl font-semibold tracking-tight">Client setup</h1>
	<p class="mt-2 text-sm text-mist">
		ClosedRouter speaks both OpenAI Chat Completions and Anthropic Messages. Auth is a Bearer token
		issued here.
	</p>

	<section class="mt-8">
		<h2 class="text-lg font-medium">OpenAI SDK</h2>
		<pre class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{openai}</pre>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">Anthropic SDK</h2>
		<pre class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{anthropic}</pre>
	</section>
	<section class="mt-8">
		<h2 class="text-lg font-medium">Cursor</h2>
		<pre class="mt-3 overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px]">{cursor}</pre>
		<p class="mt-3 text-sm text-mist">
			Override OpenAI base URL in Cursor settings, or an OpenAI-compatible model provider, to this
			gateway.
		</p>
	</section>
</div>
