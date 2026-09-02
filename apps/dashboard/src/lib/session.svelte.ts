import { browser } from '$app/environment';

const URL_KEY = 'closedrouter.gatewayUrl';
const TOKEN_KEY = 'closedrouter.adminToken';

export const session = $state({
	gatewayUrl: 'http://localhost:8080',
	adminToken: '',
	hydrated: false
});

export function hydrateSession(defaultUrl: string): void {
	if (!browser) {
		session.gatewayUrl = defaultUrl.replace(/\/$/, '');
		session.hydrated = true;
		return;
	}
	session.gatewayUrl = (localStorage.getItem(URL_KEY) ?? defaultUrl).replace(/\/$/, '');
	session.adminToken = localStorage.getItem(TOKEN_KEY) ?? '';
	session.hydrated = true;
}

export function persistSession(): void {
	if (!browser) return;
	localStorage.setItem(URL_KEY, session.gatewayUrl.replace(/\/$/, ''));
	localStorage.setItem(TOKEN_KEY, session.adminToken);
}

export function clearSession(): void {
	session.adminToken = '';
	persistSession();
}

export function isAuthed(): boolean {
	return session.adminToken.trim().length > 0;
}
