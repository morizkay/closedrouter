import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Link } from '@tanstack/react-router';
import { adminRequest } from '../lib/api';
import { getSession } from '../lib/session';
import type { StatusPayload } from '../lib/types';

export function OverviewPage(): React.JSX.Element {
	const { gatewayUrl } = getSession();
	const [copied, setCopied] = useState(false);
	const statusQuery = useQuery({
		queryKey: ['status'],
		queryFn: () => adminRequest<StatusPayload>('/admin/v1/status')
	});
	const status = statusQuery.data;
	const baseUrl = `${gatewayUrl}/v1`;

	async function copyUrl(): Promise<void> {
		await navigator.clipboard.writeText(baseUrl);
		setCopied(true);
		window.setTimeout(() => setCopied(false), 1800);
	}

	return (
		<div className="page-wrap">
			<div className="page-header">
				<div>
					<p className="eyebrow">Overview</p>
					<h1 className="page-title">Gateway status</h1>
					<p className="page-description">
						A quick read on your routing layer, model catalog, and client connection details.
					</p>
				</div>
				<button
					type="button"
					className="button-secondary"
					onClick={() => void statusQuery.refetch()}
					disabled={statusQuery.isFetching}
				>
					{statusQuery.isFetching ? 'Refreshing…' : 'Refresh status'}
				</button>
			</div>

			{statusQuery.isError && (
				<div className="feedback error" role="alert">
					<span>
						{statusQuery.error instanceof Error
							? statusQuery.error.message
							: 'Failed to load status'}
					</span>
					<button type="button" className="button-quiet" onClick={() => void statusQuery.refetch()}>
						Retry
					</button>
				</div>
			)}

			{statusQuery.isLoading && (
				<div className="stats-grid page-section" aria-label="Loading status">
					{[1, 2, 3].map((item) => (
						<div className="surface stat-card loading-card" key={item} />
					))}
				</div>
			)}

			{status && (
				<>
					<div className="stats-grid page-section">
						<StatCard label="Active API keys" value={status.keys} detail="client credentials" />
						<StatCard
							label="Connected providers"
							value={status.providers}
							detail="upstream routes"
						/>
						<StatCard label="Available models" value={status.models} detail="friendly IDs" />
					</div>

					<div className="two-column page-section">
						<section className="surface card-padding">
							<div className="card-topline">
								<div>
									<div className="inline-status">
										<span className="status-dot" /> Gateway is operational
									</div>
									<p className="card-copy">
										Your gateway is accepting admin requests and ready to route client traffic
										through the configured catalog.
									</p>
								</div>
								<span className="badge success">{status.status}</span>
							</div>
							<div className="detail-grid">
								<Detail label="Listening on" value={`${status.host}:${status.port}`} />
								<Detail label="Version" value={`v${status.version}`} />
								<Detail label="Embeddings" value={`${status.embedding_dim ?? '—'} dimensions`} />
							</div>
						</section>

						<section className="surface card-padding accent-card">
							<p className="eyebrow">Next step</p>
							<h2 className="card-heading">Ship your first request</h2>
							<p className="card-copy">
								Create a key, map a model, then test the route from the playground.
							</p>
							<div className="card-actions">
								<Link to="/keys" className="button-primary">
									Create API key
								</Link>
								<Link to="/playground" className="button-secondary">
									Open playground
								</Link>
							</div>
						</section>
					</div>

					<section className="surface card-padding page-section">
						<div className="section-row">
							<div>
								<p className="eyebrow">Client connection</p>
								<h2>Use your OpenAI-compatible base URL</h2>
								<p className="card-copy">
									Point Cursor, the OpenAI SDK, or LangChain at this endpoint with a ClosedRouter
									key.
								</p>
							</div>
							<Link to="/docs" className="button-quiet">
								View setup guides →
							</Link>
						</div>
						<div className="connection-row">
							<div className="code-box">
								<span className="mono-label">Base URL</span>
								<strong>{baseUrl}</strong>
							</div>
							<button type="button" className="button-secondary" onClick={() => void copyUrl()}>
								{copied ? 'Copied' : 'Copy URL'}
							</button>
						</div>
						{status.langfuse && (
							<p className="success-copy">Langfuse export is enabled for this gateway.</p>
						)}
					</section>
				</>
			)}
		</div>
	);
}

function StatCard({
	label,
	value,
	detail
}: {
	label: string;
	value: number;
	detail: string;
}): React.JSX.Element {
	return (
		<div className="surface stat-card">
			<div className="stat-card-head">
				<span>{label}</span>
				<span className="status-dot" />
			</div>
			<div className="stat-value">{value}</div>
			<div className="stat-detail">{detail}</div>
		</div>
	);
}

function Detail({ label, value }: { label: string; value: string }): React.JSX.Element {
	return (
		<div>
			<p className="mono-label">{label}</p>
			<p className="detail-value">{value}</p>
		</div>
	);
}
