<script lang="ts">
	import { adminRequest } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import type { StatusPayload } from '$lib/types';
	import { onMount } from 'svelte';

	let status = $state<StatusPayload | null>(null);
	let error = $state('');
	let copied = $state('');

	async function copy(label: string, text: string): Promise<void> {
		await navigator.clipboard.writeText(text);
		copied = label;
	}

	const cursorBase = $derived(`${session.gatewayUrl}/v1`);

	async function load(): Promise<void> {
		try {
			status = await adminRequest<StatusPayload>('/admin/v1/status');
			error = '';
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to load status';
		}
	}

	onMount(() => {
		if (session.adminToken) {
			void load();
		}
	});
</script>

<div class="mx-auto max-w-5xl">
	<p class="text-xs tracking-[0.28em] text-mist uppercase">Overview</p>
	<h1 class="mt-2 text-3xl font-semibold tracking-tight">Gateway status</h1>

	{#if error}
		<p class="mt-6 text-sm text-danger">{error}</p>
	{/if}

	{#if status}
		<div class="mt-8 grid gap-4 sm:grid-cols-3">
			{#each [{ label: 'Active keys', value: String(status.keys) }, { label: 'Providers', value: String(status.providers) }, { label: 'Models', value: String(status.models) }] as card (card.label)}
				<div class="rounded-2xl border border-line bg-panel p-5">
					<p class="text-xs text-mist">{card.label}</p>
					<p class="mt-2 font-mono text-3xl">{card.value}</p>
				</div>
			{/each}
		</div>
		<div class="mt-6 rounded-2xl border border-line bg-panel p-5 text-sm text-mist">
			<p>
				Listening on
				<span class="font-mono text-paper">{status.host}:{status.port}</span>
				· v{status.version}
			</p>
			<p class="mt-2">
				Clients use <span class="font-mono text-acid">{session.gatewayUrl}/v1</span> with a ClosedRouter
				API key.
			</p>
			{#if status.langfuse}
				<p class="mt-2">Langfuse export is on.</p>
			{/if}
		</div>
		<div class="mt-6 rounded-2xl border border-acid/30 bg-panel p-5">
			<p class="text-xs tracking-[0.2em] text-mist uppercase">Cursor</p>
			<p class="mt-2 text-sm text-mist">Settings → Models → OpenAI override</p>
			<p class="mt-3 font-mono text-sm">Base URL: {cursorBase}</p>
			<p class="mt-1 font-mono text-sm text-mist">API key: a ClosedRouter sk-cr-… key</p>
			<div class="mt-3 flex flex-wrap gap-3">
				<button type="button" class="text-sm text-acid" onclick={() => copy('url', cursorBase)}>
					{copied === 'url' ? 'Copied URL' : 'Copy base URL'}
				</button>
			</div>
		</div>
	{/if}
</div>
