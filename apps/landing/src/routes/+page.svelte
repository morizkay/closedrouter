<script lang="ts">
	import favicon from '$lib/assets/favicon.svg';
	import { resolve } from '$app/paths';

	type Tab = 'openai' | 'anthropic' | 'cursor';

	let tab = $state<Tab>('openai');

	const snippets: Record<Tab, string> = {
		openai: `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:8080/v1",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

const res = await client.chat.completions.create({
  model: "llama3",
  messages: [{ role: "user", content: "hello" }],
});`,
		anthropic: `import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "http://localhost:8080",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

const res = await client.messages.create({
  model: "llama3",
  max_tokens: 256,
  messages: [{ role: "user", content: "hello" }],
});`,
		cursor: `{
  "OpenAI": {
    "apiBase": "http://localhost:8080/v1",
    "apiKey": "sk-cr-your-closedrouter-key",
    "model": "llama3"
  }
}`
	};

	const docker = `git clone https://github.com/morizkay/closedrouter.git
cd closedrouter
cp .env.example .env
docker compose up --build`;
</script>

<svelte:head>
	<title>ClosedRouter — OpenRouter, but local</title>
	<meta
		name="description"
		content="Self-hosted LLM gateway with OpenAI- and Anthropic-compatible APIs. Your models, your keys, your network."
	/>
	<meta property="og:title" content="ClosedRouter — OpenRouter, but local" />
	<meta
		property="og:description"
		content="An LLM API gateway you run yourself. Docker, privacy, OpenAI + Anthropic SDKs."
	/>
	<meta property="og:type" content="website" />
	<link rel="icon" href={favicon} />
</svelte:head>

<div class="relative overflow-hidden">
	<div
		class="pointer-events-none absolute inset-0 opacity-40"
		style="background-image: radial-gradient(rgb(255 255 255 / 0.06) 1px, transparent 1px); background-size: 22px 22px;"
	></div>

	<header class="relative z-10 mx-auto flex max-w-6xl items-center justify-between px-6 py-6">
		<a href={resolve('/')} class="flex items-center gap-2.5">
			<img src={favicon} alt="ClosedRouter" class="h-8 w-8" />
			<span class="text-sm font-semibold tracking-[0.18em] uppercase">ClosedRouter</span>
		</a>
		<nav class="hidden items-center gap-8 text-sm text-mist md:flex">
			<a href="#how" class="hover:text-paper">How it works</a>
			<a href="#sdk" class="hover:text-paper">SDKs</a>
			<a href="#docker" class="hover:text-paper">Docker</a>
			<a
				href="https://github.com/morizkay/closedrouter"
				class="rounded-full border border-line px-4 py-1.5 text-paper hover:border-acid"
			>
				GitHub
			</a>
		</nav>
	</header>

	<section class="relative z-10 mx-auto max-w-6xl px-6 pb-24 pt-10 md:pt-20">
		<p class="mb-5 text-xs font-medium tracking-[0.28em] text-acid uppercase">
			Private LLM gateway
		</p>
		<h1 class="max-w-3xl text-5xl leading-[0.95] font-semibold tracking-tight md:text-7xl">
			OpenRouter, but local.
		</h1>
		<p class="mt-6 max-w-xl text-lg text-mist">
			ClosedRouter is an LLM API gateway you run yourself. Point the OpenAI SDK, Anthropic SDK, or
			Cursor at a box you control — Ollama, vLLM, Groq, or the frontier APIs, on your network.
		</p>
		<div class="mt-10 flex flex-wrap gap-3">
			<a
				href="#docker"
				class="rounded-full bg-acid px-5 py-2.5 text-sm font-semibold text-ink hover:brightness-95"
			>
				Self-host with Docker
			</a>
			<a
				href="#sdk"
				class="rounded-full border border-line px-5 py-2.5 text-sm text-paper hover:border-paper/40"
			>
				See the APIs
			</a>
		</div>

		<div
			class="mt-16 overflow-hidden rounded-2xl border border-line bg-panel/80 shadow-[0_0_80px_rgb(198_255_74_/_0.06)] backdrop-blur"
		>
			<div class="flex items-center gap-2 border-b border-line px-4 py-3 text-xs text-mist">
				<span class="h-2.5 w-2.5 rounded-full bg-white/15"></span>
				<span class="h-2.5 w-2.5 rounded-full bg-white/15"></span>
				<span class="h-2.5 w-2.5 rounded-full bg-acid/80"></span>
				<span class="ml-3 font-mono">POST /v1/chat/completions</span>
			</div>
			<pre class="overflow-x-auto p-5 font-mono text-[13px] leading-relaxed text-paper/90">{`curl http://localhost:8080/v1/chat/completions \\
  -H "Authorization: Bearer sk-cr-..." \\
  -H "Content-Type: application/json" \\
  -d '{"model":"llama3","messages":[{"role":"user","content":"hello"}]}'`}</pre>
		</div>
	</section>
</div>

<section class="mx-auto grid max-w-6xl gap-4 px-6 pb-24 md:grid-cols-4">
	{#each [
		{
			k: '01',
			t: 'OpenAI compatible',
			d: 'Drop-in /v1/chat/completions and /v1/models. Streaming or not.'
		},
		{
			k: '02',
			t: 'Anthropic compatible',
			d: 'Speak /v1/messages natively. Same catalog, same keys.'
		},
		{
			k: '03',
			t: 'Self-hosted',
			d: 'One docker compose. SQLite. No vendor lock-in, no traffic leaving your VPC unless you say so.'
		},
		{
			k: '04',
			t: 'Your models',
			d: 'Map llama3 or claude-sonnet to Ollama, vLLM, LM Studio, Groq, OpenAI, Anthropic.'
		}
	] as item (item.k)}
		<div class="rounded-2xl border border-line bg-panel p-5">
			<p class="font-mono text-xs text-acid">{item.k}</p>
			<h2 class="mt-3 text-lg font-medium">{item.t}</h2>
			<p class="mt-2 text-sm leading-relaxed text-mist">{item.d}</p>
		</div>
	{/each}
</section>

<section id="how" class="mx-auto max-w-6xl px-6 pb-24">
	<p class="text-xs tracking-[0.28em] text-mist uppercase">How it works</p>
	<h2 class="mt-3 max-w-xl text-3xl font-semibold tracking-tight md:text-4xl">
		One front door. Many upstreams.
	</h2>
	<ol class="mt-10 grid gap-4 md:grid-cols-3">
		{#each [
			{
				n: '1',
				t: 'Issue keys',
				d: 'The dashboard mints ClosedRouter API keys. Clients never see upstream credentials.'
			},
			{
				n: '2',
				t: 'Map models',
				d: 'Friendly IDs route to an OpenAI-compat or Anthropic-compat base URL and upstream model name.'
			},
			{
				n: '3',
				t: 'Translate & proxy',
				d: 'Incoming OpenAI or Anthropic requests are converted when the upstream speaks the other dialect, including streams.'
			}
		] as step (step.n)}
			<li class="rounded-2xl border border-line bg-panel p-6">
				<div
					class="flex h-8 w-8 items-center justify-center rounded-full bg-acid font-mono text-sm text-ink"
				>
					{step.n}
				</div>
				<h3 class="mt-4 text-lg font-medium">{step.t}</h3>
				<p class="mt-2 text-sm text-mist">{step.d}</p>
			</li>
		{/each}
	</ol>
</section>

<section id="sdk" class="mx-auto max-w-6xl px-6 pb-24">
	<p class="text-xs tracking-[0.28em] text-mist uppercase">SDKs</p>
	<h2 class="mt-3 text-3xl font-semibold tracking-tight md:text-4xl">Point existing clients at it.</h2>
	<div class="mt-8 overflow-hidden rounded-2xl border border-line bg-panel">
		<div class="flex gap-1 border-b border-line p-2">
			{#each [
				{ id: 'openai' as const, label: 'OpenAI SDK' },
				{ id: 'anthropic' as const, label: 'Anthropic SDK' },
				{ id: 'cursor' as const, label: 'Cursor' }
			] as t (t.id)}
				<button
					type="button"
					class="rounded-lg px-3 py-1.5 text-sm {tab === t.id
						? 'bg-white/10 text-paper'
						: 'text-mist hover:text-paper'}"
					onclick={() => (tab = t.id)}
				>
					{t.label}
				</button>
			{/each}
		</div>
		<pre class="overflow-x-auto p-5 font-mono text-[13px] leading-relaxed text-paper/90">{snippets[tab]}</pre>
	</div>
</section>

<section id="docker" class="mx-auto max-w-6xl px-6 pb-24">
	<div class="grid items-center gap-10 md:grid-cols-2">
		<div>
			<p class="text-xs tracking-[0.28em] text-mist uppercase">Deploy</p>
			<h2 class="mt-3 text-3xl font-semibold tracking-tight md:text-4xl">
				One compose file. Gateway on 8080, dashboard on 3000.
			</h2>
			<p class="mt-4 text-mist">
				Rust gateway, SvelteKit admin, SQLite on a volume. Bind
				<code class="font-mono text-acid">0.0.0.0:$PORT</code> for cloud hosts. The marketing site
				on this page is what you deploy to Vercel — the gateway stays self-hosted.
			</p>
		</div>
		<pre
			class="overflow-x-auto rounded-2xl border border-line bg-panel p-5 font-mono text-[13px] leading-relaxed text-paper/90">{docker}</pre>
	</div>
</section>

<footer class="border-t border-line">
	<div class="mx-auto flex max-w-6xl flex-col gap-4 px-6 py-10 text-sm text-mist md:flex-row md:items-center md:justify-between">
		<p>ClosedRouter — your models, your keys, your network.</p>
		<div class="flex gap-6">
			<a href="https://github.com/morizkay/closedrouter" class="hover:text-paper">GitHub</a>
			<a href="#docker" class="hover:text-paper">Self-host</a>
			<a href="https://github.com/morizkay/closedrouter#readme" class="hover:text-paper">Docs</a>
		</div>
	</div>
</footer>
