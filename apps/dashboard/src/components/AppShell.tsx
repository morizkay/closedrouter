import { Link, useNavigate, useRouterState } from '@tanstack/react-router';
import { clearSession, getSession } from '../lib/session';

const links = [
	{ to: '/', label: 'Overview' },
	{ to: '/keys', label: 'API keys' },
	{ to: '/models', label: 'Models' },
	{ to: '/playground', label: 'Playground' },
	{ to: '/docs', label: 'Docs' }
] as const;

export function AppShell({ children }: { children: React.ReactNode }): React.JSX.Element {
	const navigate = useNavigate();
	const pathname = useRouterState({ select: (state) => state.location.pathname });
	const { gatewayUrl } = getSession();

	function signOut(): void {
		clearSession();
		void navigate({ to: '/' });
	}

	return (
		<div className="app-shell">
			<header className="app-header">
				<div className="app-header-inner">
					<Link to="/" className="brand-lockup">
						<img
							src="/favicon.svg"
							alt="ClosedRouter"
							className="brand-mark"
							width="38"
							height="38"
						/>
						<span>
							<span className="brand-name">ClosedRouter</span>
							<span className="brand-caption">Yours on your own</span>
						</span>
					</Link>

					<nav className="main-nav" aria-label="Main navigation">
						{links.map((link) => (
							<Link
								key={link.to}
								to={link.to}
								className={pathname === link.to ? 'nav-link active' : 'nav-link'}
								aria-current={pathname === link.to ? 'page' : undefined}
							>
								{link.label}
							</Link>
						))}
					</nav>

					<div className="header-actions">
						<div className="gateway-chip" title={gatewayUrl}>
							<span className="status-dot" />
							<span>Gateway connected</span>
						</div>
						<button type="button" className="account-button" onClick={signOut}>
							Disconnect
						</button>
					</div>
				</div>
			</header>
			<main>{children}</main>
		</div>
	);
}
