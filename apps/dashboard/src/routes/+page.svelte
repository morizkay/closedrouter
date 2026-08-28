<script lang="ts">
	import { adminRequest } from '$lib/api';
	import { session } from '$lib/session.svelte';
	import type { StatusPayload } from '$lib/types';
	import { onMount } from 'svelte';

	let status = $state<StatusPayload | null>(null);
	let error = $state('');

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
			{#each [
				{ label: 'Active keys', value: String(status.keys) },
				{ label: 'Providers', value: String(status.providers) },
				{ label: 'Models', value: String(status.models) }
			] as card (card.label)}
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
		</div>
	{/if}
</div>
