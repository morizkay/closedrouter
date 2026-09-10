import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { adminRequest } from '../lib/api';
import type { ListResponse, Model, ModelCapability, Provider, ProviderKind } from '../lib/types';

export function ModelsPage(): React.JSX.Element {
	const queryClient = useQueryClient();
	const providersQuery = useQuery({
		queryKey: ['providers'],
		queryFn: () => adminRequest<ListResponse<Provider>>('/admin/v1/providers')
	});
	const modelsQuery = useQuery({
		queryKey: ['models'],
		queryFn: () => adminRequest<ListResponse<Model>>('/admin/v1/models')
	});
	const providers = providersQuery.data?.data ?? [];
	const models = modelsQuery.data?.data ?? [];
	const [providerName, setProviderName] = useState('Ollama');
	const [providerKind, setProviderKind] = useState<ProviderKind>('openai');
	const [baseUrl, setBaseUrl] = useState('http://127.0.0.1:11434/v1');
	const [providerKey, setProviderKey] = useState('');
	const [modelId, setModelId] = useState('llama3');
	const [modelProviderId, setModelProviderId] = useState('');
	const [upstreamModel, setUpstreamModel] = useState('llama3.2');
	const [displayName, setDisplayName] = useState('Llama 3');
	const [capability, setCapability] = useState<ModelCapability>('chat');
	const [feedback, setFeedback] = useState('');

	useEffect(() => {
		if (!modelProviderId && providers[0]) setModelProviderId(providers[0].id);
	}, [modelProviderId, providers]);

	const reloadCatalog = async (): Promise<void> => {
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: ['providers'] }),
			queryClient.invalidateQueries({ queryKey: ['models'] })
		]);
	};
	const providerMutation = useMutation({
		mutationFn: () =>
			adminRequest('/admin/v1/providers', {
				method: 'POST',
				body: JSON.stringify({
					name: providerName.trim(),
					kind: providerKind,
					base_url: baseUrl.trim(),
					api_key: providerKey || null
				})
			}),
		onSuccess: async () => {
			setProviderKey('');
			setFeedback('Provider added.');
			await reloadCatalog();
		}
	});
	const modelMutation = useMutation({
		mutationFn: () =>
			adminRequest('/admin/v1/models', {
				method: 'POST',
				body: JSON.stringify({
					id: modelId.trim(),
					provider_id: modelProviderId,
					upstream_model: upstreamModel.trim(),
					display_name: displayName.trim(),
					capability
				})
			}),
		onSuccess: async () => {
			setFeedback('Model mapped.');
			await reloadCatalog();
		}
	});
	const deleteProvider = useMutation({
		mutationFn: (id: string) =>
			adminRequest(`/admin/v1/providers/${encodeURIComponent(id)}`, { method: 'DELETE' }),
		onSuccess: async () => {
			setFeedback('Provider deleted.');
			await reloadCatalog();
		}
	});
	const deleteModel = useMutation({
		mutationFn: (id: string) =>
			adminRequest(`/admin/v1/models/${encodeURIComponent(id)}`, { method: 'DELETE' }),
		onSuccess: async () => {
			setFeedback('Model deleted.');
			await reloadCatalog();
		}
	});
	const error =
		providersQuery.error ??
		modelsQuery.error ??
		providerMutation.error ??
		modelMutation.error ??
		deleteProvider.error ??
		deleteModel.error;
	const loading = providersQuery.isLoading || modelsQuery.isLoading;

	function handleProvider(event: React.FormEvent<HTMLFormElement>): void {
		event.preventDefault();
		setFeedback('');
		providerMutation.mutate();
	}
	function handleModel(event: React.FormEvent<HTMLFormElement>): void {
		event.preventDefault();
		setFeedback('');
		if (!modelProviderId) {
			setFeedback('Add a provider before mapping a model.');
			return;
		}
		modelMutation.mutate();
	}
	function confirmDelete(message: string, action: () => void): void {
		if (window.confirm(message)) action();
	}

	return (
		<div className="page-wrap">
			<div className="page-header">
				<div>
					<p className="eyebrow">Catalog</p>
					<h1 className="page-title">Providers and models</h1>
					<p className="page-description">
						Connect an upstream, then expose a stable friendly model ID to every client. Credentials
						remain server-side.
					</p>
				</div>
				<button
					type="button"
					className="button-secondary"
					onClick={() => void reloadCatalog()}
					disabled={loading}
				>
					{loading ? 'Refreshing…' : 'Refresh catalog'}
				</button>
			</div>
			{(error || feedback) && (
				<div className={`feedback ${error ? 'error' : 'success'}`} role="status">
					{error instanceof Error ? error.message : feedback}
				</div>
			)}
			<div className="two-column page-section">
				<form className="surface card-padding" onSubmit={handleProvider}>
					<p className="eyebrow">Step 01</p>
					<h2 className="card-heading">Add a provider</h2>
					<p className="card-copy">
						Where should ClosedRouter send requests? OpenAI-compatible endpoints cover Ollama, vLLM,
						Groq, and more.
					</p>
					<div className="form-grid">
						<label className="form-label wide">
							<span>Provider name</span>
							<input
								value={providerName}
								onChange={(event) => setProviderName(event.target.value)}
								placeholder="e.g. Ollama local"
							/>
						</label>
						<label className="form-label">
							<span>Protocol</span>
							<select
								value={providerKind}
								onChange={(event) => setProviderKind(event.target.value as ProviderKind)}
							>
								<option value="openai">OpenAI-compatible</option>
								<option value="anthropic">Anthropic-compatible</option>
								<option value="deepseek">DeepSeek</option>
								<option value="glm">GLM / Zhipu</option>
							</select>
						</label>
						<label className="form-label">
							<span>
								API key <em>optional</em>
							</span>
							<input
								type="password"
								value={providerKey}
								onChange={(event) => setProviderKey(event.target.value)}
								autoComplete="off"
							/>
						</label>
						<label className="form-label wide">
							<span>Base URL</span>
							<input
								value={baseUrl}
								onChange={(event) => setBaseUrl(event.target.value)}
								spellCheck={false}
							/>
						</label>
					</div>
					<button type="submit" className="button-primary" disabled={providerMutation.isPending}>
						{providerMutation.isPending ? 'Saving provider…' : 'Save provider'}
					</button>
				</form>
				<form className="surface card-padding" onSubmit={handleModel}>
					<p className="eyebrow">Step 02</p>
					<h2 className="card-heading">Map a model</h2>
					<p className="card-copy">
						Choose the short ID your clients will request and map it to the upstream model name.
					</p>
					<div className="form-grid">
						<label className="form-label">
							<span>Friendly ID</span>
							<input
								value={modelId}
								onChange={(event) => setModelId(event.target.value)}
								placeholder="llama3"
							/>
						</label>
						<label className="form-label">
							<span>Provider</span>
							<select
								value={modelProviderId}
								onChange={(event) => setModelProviderId(event.target.value)}
								disabled={!providers.length}
							>
								<option value="" disabled>
									{providers.length ? 'Select provider' : 'Add a provider first'}
								</option>
								{providers.map((provider) => (
									<option value={provider.id} key={provider.id}>
										{provider.name}
									</option>
								))}
							</select>
						</label>
						<label className="form-label">
							<span>Upstream model</span>
							<input
								value={upstreamModel}
								onChange={(event) => setUpstreamModel(event.target.value)}
							/>
						</label>
						<label className="form-label">
							<span>Capability</span>
							<select
								value={capability}
								onChange={(event) => setCapability(event.target.value as ModelCapability)}
							>
								<option value="chat">Chat completion</option>
								<option value="embedding">Embedding</option>
							</select>
						</label>
						<label className="form-label wide">
							<span>Display name</span>
							<input value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
						</label>
					</div>
					<button
						type="submit"
						className="button-primary"
						disabled={modelMutation.isPending || !providers.length}
					>
						{modelMutation.isPending ? 'Saving model…' : 'Save model mapping'}
					</button>
				</form>
			</div>
			<div className="section-row page-section">
				<div>
					<p className="eyebrow">Connected infrastructure</p>
					<h2>Your routing catalog</h2>
				</div>
				<div className="catalog-count">
					<span>{providers.length} providers</span>
					<span>{models.length} models</span>
				</div>
			</div>
			<section className="catalog-section">
				<p className="mono-label">Providers</p>
				{providers.length ? (
					<div className="surface list-card">
						{providers.map((provider) => (
							<div className="list-row" key={provider.id}>
								<div className="list-main">
									<div className="list-title">
										{provider.name} <span className="badge purple">{provider.kind}</span>
										{provider.has_api_key && <span className="key-note">key configured</span>}
									</div>
									<div className="list-subtitle">{provider.base_url}</div>
								</div>
								<button
									type="button"
									className="button-quiet danger-text"
									onClick={() =>
										confirmDelete(
											'Delete this provider? Models mapped to it may stop working.',
											() => deleteProvider.mutate(provider.id)
										)
									}
								>
									Delete
								</button>
							</div>
						))}
					</div>
				) : (
					<div className="surface empty-inline">
						No providers connected yet. Start with the form above.
					</div>
				)}
			</section>
			<section className="catalog-section">
				<p className="mono-label">Models</p>
				{models.length ? (
					<div className="surface list-card">
						{models.map((model) => (
							<div className="list-row" key={model.id}>
								<div className="list-main">
									<div className="list-title">
										<code className="model-id">{model.id}</code>{' '}
										<span className="arrow-label">→ {model.upstream_model}</span>
									</div>
									<div className="list-subtitle">
										{model.display_name} via {model.provider_name}
									</div>
								</div>
								<div className="row-actions">
									<span className="badge coral">{model.capability}</span>
									<button
										type="button"
										className="button-quiet danger-text"
										onClick={() =>
											confirmDelete('Delete this model mapping?', () =>
												deleteModel.mutate(model.id)
											)
										}
									>
										Delete
									</button>
								</div>
							</div>
						))}
					</div>
				) : (
					<div className="surface empty-inline">
						No model mappings yet. Add a provider, then map the model ID clients should use.
					</div>
				)}
			</section>
		</div>
	);
}
