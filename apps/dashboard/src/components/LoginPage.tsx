import { useState } from 'react';
import { adminRequest, ApiClientError } from '../lib/api';
import { getSession, persistSession, updateSession } from '../lib/session';
import type { StatusPayload } from '../lib/types';

export function LoginPage(): React.JSX.Element {
	const session = getSession();
	const [gatewayUrl, setGatewayUrl] = useState(session.gatewayUrl);
	const [adminToken, setAdminToken] = useState(session.adminToken);
	const [error, setError] = useState('');
	const [busy, setBusy] = useState(false);

	async function connect(event: React.FormEvent<HTMLFormElement>): Promise<void> {
		event.preventDefault();
		setError('');
		setBusy(true);
		const normalizedUrl = gatewayUrl.replace(/\/$/, '');
		try {
			await adminRequest<StatusPayload>(
				'/admin/v1/status',
				{},
				{ gatewayUrl: normalizedUrl, adminToken }
			);
			updateSession({ gatewayUrl: normalizedUrl, adminToken });
			persistSession();
		} catch (err) {
			setError(err instanceof ApiClientError ? err.message : 'Could not reach the gateway');
		} finally {
			setBusy(false);
		}
	}

	return (
		<div className="login-layout">
			<div className="login-orb login-orb-left" />
			<div className="login-orb login-orb-right" />
			<div className="login-column">
				<div className="login-brand">
					<img
						src="/favicon.svg"
						alt="ClosedRouter"
						className="brand-mark"
						width="42"
						height="42"
					/>
					<div>
						<p className="brand-name">ClosedRouter</p>
						<p className="brand-caption">Yours on your own</p>
					</div>
				</div>
				<section className="surface login-card">
					<div className="login-card-top">
						<div>
							<p className="eyebrow">Admin console</p>
							<h1 className="login-title">Connect your gateway</h1>
						</div>
						<span className="step-badge">01 / 01</span>
					</div>
					<p className="login-copy">
						Use the admin token from your gateway environment to open the control plane. It stays in
						this browser session.
					</p>
					<form className="login-form" onSubmit={connect}>
						<label className="form-label">
							<span>Gateway URL</span>
							<input
								value={gatewayUrl}
								onChange={(event) => setGatewayUrl(event.target.value)}
								autoComplete="url"
								spellCheck={false}
							/>
							<small>The address where your gateway is listening.</small>
						</label>
						<label className="form-label">
							<span>Admin token</span>
							<input
								type="password"
								value={adminToken}
								onChange={(event) => setAdminToken(event.target.value)}
								autoComplete="current-password"
							/>
						</label>
						<button type="submit" className="button-primary login-submit" disabled={busy}>
							{busy ? 'Checking connection…' : 'Open dashboard'}
						</button>
						{error && (
							<div className="feedback error" role="alert">
								{error}
							</div>
						)}
					</form>
				</section>
				<p className="login-footnote">
					Your providers and upstream keys never leave your infrastructure.
				</p>
			</div>
		</div>
	);
}
