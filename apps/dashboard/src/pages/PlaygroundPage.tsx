import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Link } from '@tanstack/react-router';
import { adminRequest, chatCompletions } from '../lib/api';
import type { ApiKey, ListResponse, Model } from '../lib/types';

type ChatMessage = { role: 'user' | 'assistant'; content: string };

export function PlaygroundPage(): React.JSX.Element {
	const modelsQuery = useQuery({
		queryKey: ['models'],
		queryFn: () => adminRequest<ListResponse<Model>>('/admin/v1/models')
	});
	const keysQuery = useQuery({
		queryKey: ['keys'],
		queryFn: () => adminRequest<ListResponse<ApiKey>>('/admin/v1/keys')
	});
	const models = modelsQuery.data?.data ?? [];
	const activeKeys = (keysQuery.data?.data ?? []).filter((key) => !key.revoked_at);
	const [model, setModel] = useState('');
	const [apiKey, setApiKey] = useState('');
	const [input, setInput] = useState('Explain ClosedRouter in one sentence.');
	const [messages, setMessages] = useState<ChatMessage[]>([]);
	const [sending, setSending] = useState(false);
	const [error, setError] = useState('');
	const selectedModel = model || models[0]?.id || '';

	function send(event: React.FormEvent<HTMLFormElement>): void {
		event.preventDefault();
		const prompt = input.trim();
		if (!prompt || !apiKey || !selectedModel) {
			setError(
				!selectedModel
					? 'Map a model first, then choose it here.'
					: !apiKey
						? 'Paste an active API key to send a request.'
						: 'Write a prompt to continue.'
			);
			return;
		}
		const next = [...messages, { role: 'user' as const, content: prompt }];
		setMessages(next);
		setInput('');
		setSending(true);
		setError('');
		void chatCompletions({ apiKey, model: selectedModel, messages: next, stream: false })
			.then(async (response) => {
				const payload = (await response.json()) as {
					choices?: Array<{ message?: { content?: string } }>;
					error?: { message?: string };
				};
				if (!response.ok) throw new Error(payload.error?.message ?? `HTTP ${response.status}`);
				setMessages([
					...next,
					{
						role: 'assistant',
						content:
							payload.choices?.[0]?.message?.content ?? 'The gateway returned an empty response.'
					}
				]);
			})
			.catch((reason: unknown) =>
				setError(reason instanceof Error ? reason.message : 'Chat failed')
			)
			.finally(() => setSending(false));
	}

	return (
		<div className="page-wrap">
			<div className="page-header">
				<div>
					<p className="eyebrow">Try it live</p>
					<h1 className="page-title">Playground</h1>
					<p className="page-description">
						Send one real request through your gateway and see exactly what your clients will
						receive.
					</p>
				</div>
				{messages.length > 0 && (
					<button
						type="button"
						className="button-secondary"
						onClick={() => {
							setMessages([]);
							setError('');
						}}
					>
						Clear conversation
					</button>
				)}
			</div>
			{error && (
				<div className="feedback error" role="alert">
					{error}
				</div>
			)}
			<div className="two-column page-section playground-layout">
				<section className="surface card-padding">
					<p className="eyebrow">Request setup</p>
					<h2 className="card-heading">Configure a test</h2>
					<p className="card-copy">
						This uses the public OpenAI-compatible API with one of your active keys.
					</p>
					<form className="form-stack" onSubmit={send}>
						<label className="form-label">
							<span>Model</span>
							<select
								value={selectedModel}
								onChange={(event) => setModel(event.target.value)}
								disabled={modelsQuery.isLoading || !models.length}
							>
								<option value="" disabled>
									{modelsQuery.isLoading
										? 'Loading models…'
										: models.length
											? 'Select a model'
											: 'No models mapped'}
								</option>
								{models.map((item) => (
									<option value={item.id} key={item.id}>
										{item.id} · {item.display_name}
									</option>
								))}
							</select>
						</label>
						<label className="form-label">
							<span>API key</span>
							<input
								type="password"
								value={apiKey}
								onChange={(event) => setApiKey(event.target.value)}
								placeholder={activeKeys[0] ? `${activeKeys[0].key_prefix}…` : 'Paste a sk-cr-… key'}
								autoComplete="off"
							/>
						</label>
						<p className="form-hint">
							Need one? <Link to="/keys">Create an API key</Link>.
						</p>
						<label className="form-label">
							<span>Prompt</span>
							<textarea
								className="prompt-input"
								value={input}
								onChange={(event) => setInput(event.target.value)}
								placeholder="Ask your model anything…"
							/>
						</label>
						<button
							type="submit"
							className="button-primary"
							disabled={sending || modelsQuery.isLoading}
						>
							{sending ? 'Waiting for response…' : 'Send request'}
						</button>
					</form>
				</section>
				<section className="surface response-card">
					<div className="response-header">
						<div>
							<p className="eyebrow">Response</p>
							<p className="response-model">{selectedModel || 'No model selected'}</p>
						</div>
						<span className="ready-label">
							<span className="status-dot" /> READY
						</span>
					</div>
					<div className="messages">
						{messages.length ? (
							messages.map((message, index) => (
								<div className={`message ${message.role}`} key={`${message.role}-${index}`}>
									<p className="message-role">{message.role}</p>
									<p>{message.content}</p>
								</div>
							))
						) : (
							<div className="response-empty">
								<div className="empty-icon">→</div>
								<h3>Your response will appear here</h3>
								<p>Choose a model, paste a key, and send a prompt to test the full route.</p>
							</div>
						)}
					</div>
					<div className="response-footer">
						Requests are sent directly to <code>/v1/chat/completions</code> through your gateway.
					</div>
				</section>
			</div>
		</div>
	);
}
