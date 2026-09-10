import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from '@tanstack/react-router';
import { hydrateSession } from './lib/session';
import { router } from './router';
import './styles.css';

const defaultGatewayUrl =
	window.__CLOSEDROUTER_CONFIG__?.gatewayUrl ||
	import.meta.env.PUBLIC_GATEWAY_URL ||
	'http://localhost:8080';
hydrateSession(defaultGatewayUrl);

const queryClient = new QueryClient({
	defaultOptions: {
		queries: {
			staleTime: 15_000,
			refetchOnWindowFocus: false
		}
	}
});

createRoot(document.getElementById('root')!).render(
	<StrictMode>
		<QueryClientProvider client={queryClient}>
			<RouterProvider router={router} />
		</QueryClientProvider>
	</StrictMode>
);
