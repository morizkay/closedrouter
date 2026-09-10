import { Outlet, createRootRoute, createRoute, createRouter } from '@tanstack/react-router';
import { useSession } from './lib/session';
import { AppShell } from './components/AppShell';
import { LoginPage } from './components/LoginPage';
import { OverviewPage } from './pages/OverviewPage';
import { KeysPage } from './pages/KeysPage';
import { ModelsPage } from './pages/ModelsPage';
import { PlaygroundPage } from './pages/PlaygroundPage';
import { DocsPage } from './pages/DocsPage';

function RootLayout(): React.JSX.Element {
	const { hydrated, adminToken } = useSession();
	if (!hydrated) return <div className="page-loading" aria-label="Loading dashboard" />;
	if (!adminToken) return <LoginPage />;
	return (
		<AppShell>
			<Outlet />
		</AppShell>
	);
}

const rootRoute = createRootRoute({ component: RootLayout });
const overviewRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/',
	component: OverviewPage
});
const keysRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/keys',
	component: KeysPage
});
const modelsRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/models',
	component: ModelsPage
});
const playgroundRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/playground',
	component: PlaygroundPage
});
const docsRoute = createRoute({
	getParentRoute: () => rootRoute,
	path: '/docs',
	component: DocsPage
});

const routeTree = rootRoute.addChildren([
	overviewRoute,
	keysRoute,
	modelsRoute,
	playgroundRoute,
	docsRoute
]);

export const router = createRouter({ routeTree });

declare module '@tanstack/react-router' {
	interface Register {
		router: typeof router;
	}
}
