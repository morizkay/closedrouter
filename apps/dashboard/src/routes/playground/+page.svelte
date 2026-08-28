<script lang="ts">
	import { adminRequest, chatCompletions } from '$lib/api';
	import type { ApiKey, ListResponse, Model } from '$lib/types';
	import { onMount } from 'svelte';

	type ChatRole = 'user' | 'assistant';
	type ChatMessage = { role: ChatRole; content: string };

	let models = $state<Model[]>([]);
	let keys = $state<ApiKey[]>([]);
	let model = $state('');
	let apiKey = $state('');
	let input = $state('Explain ClosedRouter in one sentence.');
	let messages = $state<ChatMessage[]>([]);
	let error = $state('');
	let sending = $state(false);

	onMount(() => {
		void Promise.all([
			adminRequest<ListResponse<Model>>('/admin/v1/models'),
			adminRequest<ListResponse<ApiKey>>('/admin/v1/keys')
		])
			.then(([m, k]) => {
				models = m.data;
				keys = k.data.filter((item) => !item.revoked_at);
				if (!model && models[0]) model = models[0].id;
			})
			.catch((err: unknown) => {
				error = err instanceof Error ? err.message : 'Failed to load playground';
			});
	});

	async function send(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		if (!input.trim() || !apiKey || !model) {
			error = 'Model, API key, and a prompt are required';
			return;
		}
		const next: ChatMessage[] = [...messages, { role: 'user', content: input.trim() }];
		messages = next;
		input = '';
		sending = true;
		error = '';
		try {
			const res = await chatCompletions({
				apiKey,
				model,
				messages: next,
				stream: false
			});
			const payload = (await res.json()) as {
				choices?: Array<{ message?: { content?: string } }>;
				error?: { message?: string };
			};
			if (!res.ok) {
				throw new Error(payload.error?.message ?? `HTTP ${res.status}`);
			}
			const text = payload.choices?.[0]?.message?.content ?? '';
			messages = [...next, { role: 'assistant', content: text }];
		} catch (err) {
			error = err instanceof Error ? err.message : 'Chat failed';
		} finally {
			sending = false;
		}
	}
</script>

<div class="mx-auto flex max-w-5xl flex-col gap-6">
	<div>
		<p class="text-xs tracking-[0.28em] text-mist uppercase">Try it</p>
		<h1 class="mt-2 text-3xl font-semibold tracking-tight">Playground</h1>
		<p class="mt-2 text-sm text-mist">
			Uses the public OpenAI-compatible API with one of your keys.
		</p>
	</div>

	<form class="grid gap-3 md:grid-cols-2" onsubmit={send}>
		<label class="text-xs text-mist uppercase">
			Model
			<select class="mt-1" bind:value={model}>
				{#each models as item (item.id)}
					<option value={item.id}>{item.id}</option>
				{/each}
			</select>
		</label>
		<label class="text-xs text-mist uppercase">
			API key (paste a created key)
			<input
				class="mt-1"
				type="password"
				bind:value={apiKey}
				placeholder={keys[0] ? `${keys[0].key_prefix}…` : 'sk-cr-…'}
			/>
		</label>
		<textarea class="min-h-24 md:col-span-2" bind:value={input} aria-label="Prompt"></textarea>
		<button
			type="submit"
			class="rounded-full bg-acid px-5 py-2 text-sm font-semibold text-ink disabled:opacity-60 md:col-span-2"
			disabled={sending}
		>
			{sending ? 'Sending…' : 'Send'}
		</button>
	</form>

	{#if error}
		<p class="text-sm text-danger">{error}</p>
	{/if}

	<div class="flex flex-col gap-3">
		{#each messages as message, i (i)}
			<div class="rounded-2xl border border-line bg-panel p-4">
				<p class="text-xs tracking-wide text-mist uppercase">{message.role}</p>
				<p class="mt-2 text-sm whitespace-pre-wrap">{message.content}</p>
			</div>
		{/each}
	</div>
</div>
