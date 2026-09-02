<script lang="ts">
	import { env } from '$env/dynamic/public';
	import { adminRequest, ApiClientError } from '$lib/api';
	import { persistSession, session } from '$lib/session.svelte';
	import type { StatusPayload } from '$lib/types';

	let error = $state('');
	let busy = $state(false);

	if (!session.gatewayUrl) {
		session.gatewayUrl = (env.PUBLIC_GATEWAY_URL ?? 'http://localhost:8080').replace(/\/$/, '');
	}

	async function connect(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		error = '';
		busy = true;
		session.gatewayUrl = session.gatewayUrl.replace(/\/$/, '');
		try {
			await adminRequest<StatusPayload>('/admin/v1/status');
			persistSession();
		} catch (err) {
			error = err instanceof ApiClientError ? err.message : 'Could not reach the gateway';
		} finally {
			busy = false;
		}
	}
</script>

<div class="mx-auto flex min-h-screen max-w-md flex-col justify-center px-6">
	<p class="text-xs tracking-[0.28em] text-acid uppercase">Dashboard</p>
	<h1 class="mt-3 text-3xl font-semibold tracking-tight">Connect to your gateway</h1>
	<p class="mt-2 text-sm text-mist">
		Admin token authenticates this UI. API keys are issued separately for OpenAI and Anthropic
		clients.
	</p>
	<form class="mt-8 flex flex-col gap-4" onsubmit={connect}>
		<label class="text-xs tracking-wide text-mist uppercase">
			Gateway URL
			<input class="mt-1" bind:value={session.gatewayUrl} autocomplete="off" />
		</label>
		<label class="text-xs tracking-wide text-mist uppercase">
			Admin token
			<input class="mt-1" type="password" bind:value={session.adminToken} autocomplete="off" />
		</label>
		<button
			type="submit"
			class="rounded-full bg-acid py-2.5 text-sm font-semibold text-ink disabled:opacity-60"
			disabled={busy}
		>
			{busy ? 'Connecting…' : 'Connect'}
		</button>
		{#if error}
			<p class="text-sm text-danger">{error}</p>
		{/if}
	</form>
</div>
