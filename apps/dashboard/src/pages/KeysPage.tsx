import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { adminRequest } from '../lib/api';
import type { ApiKey, CreatedKey, ListResponse } from '../lib/types';

export function KeysPage(): React.JSX.Element {
	const queryClient = useQueryClient();
	const [name, setName] = useState('');
	const [revealed, setRevealed] = useState<string | null>(null);
	const [feedback, setFeedback] = useState('');
	const keysQuery = useQuery({
		queryKey: ['keys'],
		queryFn: () => adminRequest<ListResponse<ApiKey>>('/admin/v1/keys')
	});
	const keys = keysQuery.data?.data ?? [];
	const createMutation = useMutation({
		mutationFn: () =>
			adminRequest<CreatedKey>('/admin/v1/keys', {
				method: 'POST',
				body: JSON.stringify({ name: name.trim() })
			}),
		onSuccess: async (created) => {
			setRevealed(created.key);
			setName('');
			setFeedback('Key created. Copy the secret before leaving this page.');
			await queryClient.invalidateQueries({ queryKey: ['keys'] });
		}
	});
	const revokeMutation = useMutation({
		mutationFn: (id: string) =>
			adminRequest<void>(`/admin/v1/keys/${encodeURIComponent(id)}`, { method: 'DELETE' }),
		onSuccess: async () => {
			setFeedback('Key revoked.');
			await queryClient.invalidateQueries({ queryKey: ['keys'] });
		}
	});

	function createKey(event: React.FormEvent<HTMLFormElement>): void {
		event.preventDefault();
		setFeedback('');
		if (!name.trim()) {
			setFeedback('Give this key a name so you can recognize it later.');
			return;
		}
		createMutation.mutate();
	}

	async function copyKey(): Promise<void> {
		if (!revealed) return;
		await navigator.clipboard.writeText(revealed);
		setFeedback('Copied to clipboard.');
	}

	return (
		<div className="page-wrap">
			<PageHeader
				activeCount={keys.filter((key) => !key.revoked_at).length}
				onRefresh={() => void keysQuery.refetch()}
				refreshing={keysQuery.isFetching}
			/>
			<div className="two-column page-section keys-intro">
				<section className="surface card-padding">
					<p className="eyebrow">New credential</p>
					<h2 className="card-heading">Create an API key</h2>
					<p className="card-copy">
						Use a name that maps to a person, app, or environment. You can revoke a key at any time.
					</p>
					<form className="form-stack" onSubmit={createKey}>
						<label className="form-label">
							<span>Key name</span>
							<input
								value={name}
								onChange={(event) => setName(event.target.value)}
								placeholder="e.g. local development"
								autoComplete="off"
							/>
						</label>
						<button type="submit" className="button-primary" disabled={createMutation.isPending}>
							{createMutation.isPending ? 'Creating key…' : 'Create key'}
						</button>
					</form>
				</section>
				<section className="surface card-padding">
					<p className="eyebrow">Protect the secret</p>
					<h2 className="card-heading">Store it somewhere safe</h2>
					{revealed ? (
						<div className="secret-box">
							<strong>Your new key is ready</strong>
							<p>
								This is the only time the full secret will appear. Copy it before leaving this page.
							</p>
							<div className="secret-row">
								<code>{revealed}</code>
								<button type="button" className="button-secondary" onClick={() => void copyKey()}>
									Copy
								</button>
							</div>
						</div>
					) : (
						<div className="empty-inset">
							<strong>No secret waiting to be copied</strong>
							<p>
								New keys are hashed immediately. Keep the full value out of source control and chat
								logs.
							</p>
						</div>
					)}
				</section>
			</div>

			{(keysQuery.isError || createMutation.isError || revokeMutation.isError || feedback) && (
				<div
					className={`feedback ${keysQuery.isError || createMutation.isError || revokeMutation.isError ? 'error' : 'success'}`}
					role="status"
				>
					{keysQuery.error instanceof Error
						? keysQuery.error.message
						: createMutation.error instanceof Error
							? createMutation.error.message
							: revokeMutation.error instanceof Error
								? revokeMutation.error.message
								: feedback}
				</div>
			)}

			<div className="section-row page-section">
				<div>
					<p className="eyebrow">Inventory</p>
					<h2>Issued credentials</h2>
				</div>
				<button
					type="button"
					className="button-quiet"
					onClick={() => void keysQuery.refetch()}
					disabled={keysQuery.isFetching}
				>
					{keysQuery.isFetching ? 'Loading…' : 'Refresh list'}
				</button>
			</div>
			{keysQuery.isLoading ? (
				<div className="surface loading-panel" />
			) : keys.length ? (
				<div className="surface list-card">
					{keys.map((key) => (
						<div className="list-row" key={key.id}>
							<div className="list-main">
								<div className="list-title">{key.name}</div>
								<div className="list-subtitle">
									{key.key_prefix}… · {formatDate(key.created_at)}
								</div>
							</div>
							<div className="row-actions">
								<span className={`badge ${key.revoked_at ? 'danger' : 'success'}`}>
									{key.revoked_at ? 'Revoked' : 'Active'}
								</span>
								{!key.revoked_at && (
									<button
										type="button"
										className="button-quiet danger-text"
										onClick={() => {
											if (window.confirm('Revoke this key? Any client using it will stop working.'))
												revokeMutation.mutate(key.id);
										}}
									>
										Revoke
									</button>
								)}
							</div>
						</div>
					))}
				</div>
			) : (
				<EmptyState
					title="No API keys yet"
					copy="Create your first key above to authenticate OpenAI, Anthropic, Cursor, and LangChain clients."
				/>
			)}
		</div>
	);
}

function PageHeader({
	activeCount,
	onRefresh,
	refreshing
}: {
	activeCount: number;
	onRefresh: () => void;
	refreshing: boolean;
}): React.JSX.Element {
	return (
		<div className="page-header">
			<div>
				<p className="eyebrow">Access</p>
				<h1 className="page-title">API keys</h1>
				<p className="page-description">
					Issue client credentials for your gateway. Secrets are shown once, then only their prefix
					is retained.
				</p>
			</div>
			<div className="header-stat">
				<p className="mono-label">Active keys</p>
				<strong>{activeCount}</strong>
				<button type="button" className="button-quiet" onClick={onRefresh} disabled={refreshing}>
					{refreshing ? 'Refreshing…' : 'Refresh'}
				</button>
			</div>
		</div>
	);
}

function EmptyState({ title, copy }: { title: string; copy: string }): React.JSX.Element {
	return (
		<div className="surface empty-state">
			<div className="empty-icon">01</div>
			<h3>{title}</h3>
			<p>{copy}</p>
		</div>
	);
}

function formatDate(timestamp: number): string {
	return new Intl.DateTimeFormat(undefined, {
		month: 'short',
		day: 'numeric',
		year: 'numeric'
	}).format(timestamp * 1000);
}
