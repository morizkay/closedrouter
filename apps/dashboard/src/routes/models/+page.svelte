<script lang="ts">
	import { adminRequest } from '$lib/api';
	import type { ListResponse, Model, ModelCapability, Provider, ProviderKind } from '$lib/types';
	import { onMount } from 'svelte';

	let providers = $state<Provider[]>([]);
	let models = $state<Model[]>([]);
	let error = $state('');

	let providerName = $state('Ollama');
	let providerKind = $state<ProviderKind>('openai');
	let baseUrl = $state('http://127.0.0.1:11434/v1');
	let providerKey = $state('');

	let modelId = $state('llama3');
	let modelProviderId = $state('');
	let upstreamModel = $state('llama3.2');
	let displayName = $state('Llama 3');
	let capability = $state<ModelCapability>('chat');

	async function load(): Promise<void> {
		const [p, m] = await Promise.all([
			adminRequest<ListResponse<Provider>>('/admin/v1/providers'),
			adminRequest<ListResponse<Model>>('/admin/v1/models')
		]);
		providers = p.data;
		models = m.data;
		if (!modelProviderId && providers[0]) {
			modelProviderId = providers[0].id;
		}
	}

	onMount(() => {
		void load().catch((err: unknown) => {
			error = err instanceof Error ? err.message : 'Failed to load catalog';
		});
	});

	async function addProvider(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		error = '';
		try {
			await adminRequest('/admin/v1/providers', {
				method: 'POST',
				body: JSON.stringify({
					name: providerName,
					kind: providerKind,
					base_url: baseUrl,
					api_key: providerKey || null
				})
			});
			providerKey = '';
			await load();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Provider create failed';
		}
	}

	async function addModel(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		error = '';
		try {
			await adminRequest('/admin/v1/models', {
				method: 'POST',
				body: JSON.stringify({
					id: modelId,
					provider_id: modelProviderId,
					upstream_model: upstreamModel,
					display_name: displayName,
					capability
				})
			});
			await load();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Model create failed';
		}
	}

	async function removeProvider(id: string): Promise<void> {
		await adminRequest(`/admin/v1/providers/${id}`, { method: 'DELETE' });
		await load();
	}

	async function removeModel(id: string): Promise<void> {
		await adminRequest(`/admin/v1/models/${id}`, { method: 'DELETE' });
		await load();
	}
</script>

<div class="mx-auto max-w-5xl">
	<p class="text-xs tracking-[0.28em] text-mist uppercase">Catalog</p>
	<h1 class="mt-2 text-3xl font-semibold tracking-tight">Providers and models</h1>
	<p class="mt-2 text-sm text-mist">
		OpenAI-compat (Ollama, vLLM, Groq, OpenAI, DeepSeek, GLM) or Anthropic-compat upstreams.
		Friendly IDs are what clients request. Use capability <span class="font-mono">embedding</span> for
		Hermes.
	</p>

	{#if error}
		<p class="mt-4 text-sm text-danger">{error}</p>
	{/if}

	<div class="mt-8 grid gap-8 lg:grid-cols-2">
		<form class="rounded-2xl border border-line bg-panel p-5" onsubmit={addProvider}>
			<h2 class="font-medium">Add provider</h2>
			<label class="mt-4 block text-xs text-mist uppercase"
				>Name<input class="mt-1" bind:value={providerName} /></label
			>
			<label class="mt-3 block text-xs text-mist uppercase">
				Kind
				<select class="mt-1" bind:value={providerKind}>
					<option value="openai">openai-compat</option>
					<option value="anthropic">anthropic-compat</option>
					<option value="deepseek">deepseek</option>
					<option value="glm">glm / zhipu</option>
				</select>
			</label>
			<label class="mt-3 block text-xs text-mist uppercase"
				>Base URL<input class="mt-1" bind:value={baseUrl} /></label
			>
			<label class="mt-3 block text-xs text-mist uppercase">
				Upstream API key (optional)
				<input class="mt-1" type="password" bind:value={providerKey} />
			</label>
			<button
				type="submit"
				class="mt-5 rounded-full bg-acid px-4 py-2 text-sm font-semibold text-ink"
				>Save provider</button
			>
		</form>

		<form class="rounded-2xl border border-line bg-panel p-5" onsubmit={addModel}>
			<h2 class="font-medium">Map model ID</h2>
			<label class="mt-4 block text-xs text-mist uppercase"
				>Friendly ID<input class="mt-1" bind:value={modelId} /></label
			>
			<label class="mt-3 block text-xs text-mist uppercase">
				Provider
				<select class="mt-1" bind:value={modelProviderId}>
					{#each providers as provider (provider.id)}
						<option value={provider.id}>{provider.name}</option>
					{/each}
				</select>
			</label>
			<label class="mt-3 block text-xs text-mist uppercase"
				>Upstream model<input class="mt-1" bind:value={upstreamModel} /></label
			>
			<label class="mt-3 block text-xs text-mist uppercase"
				>Display name<input class="mt-1" bind:value={displayName} /></label
			>
			<label class="mt-3 block text-xs text-mist uppercase">
				Capability
				<select class="mt-1" bind:value={capability}>
					<option value="chat">chat</option>
					<option value="embedding">embedding</option>
				</select>
			</label>
			<button
				type="submit"
				class="mt-5 rounded-full bg-acid px-4 py-2 text-sm font-semibold text-ink"
				>Save model</button
			>
		</form>
	</div>

	<h2 class="mt-10 text-lg font-medium">Providers</h2>
	<ul class="mt-3 divide-y divide-white/10 rounded-2xl border border-line">
		{#each providers as provider (provider.id)}
			<li class="flex items-center justify-between px-4 py-3 text-sm">
				<div>
					<p>{provider.name} <span class="font-mono text-mist">({provider.kind})</span></p>
					<p class="font-mono text-xs text-mist">{provider.base_url}</p>
				</div>
				<button type="button" class="text-danger" onclick={() => removeProvider(provider.id)}
					>Delete</button
				>
			</li>
		{/each}
	</ul>

	<h2 class="mt-10 text-lg font-medium">Models</h2>
	<ul class="mt-3 divide-y divide-white/10 rounded-2xl border border-line">
		{#each models as model (model.id)}
			<li class="flex items-center justify-between px-4 py-3 text-sm">
				<div>
					<p class="font-mono">
						{model.id} <span class="text-mist">→ {model.upstream_model}</span>
					</p>
					<p class="text-xs text-mist">
						{model.display_name} via {model.provider_name} · {model.capability}
					</p>
				</div>
				<button type="button" class="text-danger" onclick={() => removeModel(model.id)}
					>Delete</button
				>
			</li>
		{/each}
	</ul>
</div>
