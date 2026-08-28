<script lang="ts">
	import { adminRequest } from '$lib/api';
	import type { ApiKey, CreatedKey, ListResponse } from '$lib/types';
	import { onMount } from 'svelte';

	let keys = $state<ApiKey[]>([]);
	let name = $state('default');
	let revealed = $state<string | null>(null);
	let error = $state('');
	let copied = $state(false);

	async function load(): Promise<void> {
		const res = await adminRequest<ListResponse<ApiKey>>('/admin/v1/keys');
		keys = res.data;
	}

	onMount(() => {
		void load().catch((err: unknown) => {
			error = err instanceof Error ? err.message : 'Failed to load keys';
		});
	});

	async function createKey(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		error = '';
		try {
			const created = await adminRequest<CreatedKey>('/admin/v1/keys', {
				method: 'POST',
				body: JSON.stringify({ name })
			});
			revealed = created.key;
			copied = false;
			await load();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Create failed';
		}
	}

	async function revoke(id: string): Promise<void> {
		error = '';
		try {
			await adminRequest<undefined>(`/admin/v1/keys/${id}`, { method: 'DELETE' });
			await load();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Revoke failed';
		}
	}

	async function copyKey(): Promise<void> {
		if (!revealed) return;
		await navigator.clipboard.writeText(revealed);
		copied = true;
	}
</script>

<div class="mx-auto max-w-5xl">
	<p class="text-xs tracking-[0.28em] text-mist uppercase">Access</p>
	<h1 class="mt-2 text-3xl font-semibold tracking-tight">API keys</h1>
	<p class="mt-2 text-sm text-mist">Shown once at creation. Clients send them as Bearer tokens.</p>

	<form class="mt-8 flex max-w-lg gap-3" onsubmit={createKey}>
		<input bind:value={name} placeholder="Key name" aria-label="Key name" />
		<button type="submit" class="shrink-0 rounded-full bg-acid px-5 text-sm font-semibold text-ink">
			Create
		</button>
	</form>

	{#if revealed}
		<div class="mt-5 rounded-2xl border border-acid/30 bg-panel p-4">
			<p class="text-xs text-mist">Copy now — this secret is not stored in plaintext.</p>
			<p class="mt-2 font-mono text-sm break-all">{revealed}</p>
			<button type="button" class="mt-3 text-sm text-acid" onclick={copyKey}>
				{copied ? 'Copied' : 'Copy'}
			</button>
		</div>
	{/if}

	{#if error}
		<p class="mt-4 text-sm text-danger">{error}</p>
	{/if}

	<div class="mt-8 overflow-hidden rounded-2xl border border-line">
		<table class="w-full text-left text-sm">
			<thead class="bg-panel text-xs tracking-wide text-mist uppercase">
				<tr>
					<th class="px-4 py-3 font-medium">Name</th>
					<th class="px-4 py-3 font-medium">Prefix</th>
					<th class="px-4 py-3 font-medium">Status</th>
					<th class="px-4 py-3 font-medium"><span class="sr-only">Actions</span></th>
				</tr>
			</thead>
			<tbody>
				{#each keys as key (key.id)}
					<tr class="border-t border-line">
						<td class="px-4 py-3">{key.name}</td>
						<td class="px-4 py-3 font-mono text-mist">{key.key_prefix}…</td>
						<td class="px-4 py-3">{key.revoked_at ? 'Revoked' : 'Active'}</td>
						<td class="px-4 py-3 text-right">
							{#if !key.revoked_at}
								<button type="button" class="text-danger" onclick={() => revoke(key.id)}
									>Revoke</button
								>
							{/if}
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
