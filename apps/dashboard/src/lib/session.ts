import { useSyncExternalStore } from 'react';

const URL_KEY = 'closedrouter.gatewayUrl';
const TOKEN_KEY = 'closedrouter.adminToken';

type SessionState = {
	gatewayUrl: string;
	adminToken: string;
	hydrated: boolean;
};

let state: SessionState = {
	gatewayUrl: 'http://localhost:8080',
	adminToken: '',
	hydrated: false
};
const listeners = new Set<() => void>();

function notify(): void {
	listeners.forEach((listener) => listener());
}

export function getSession(): SessionState {
	return state;
}

export function updateSession(next: Partial<SessionState>): void {
	state = { ...state, ...next };
	notify();
}

export function useSession(): SessionState {
	return useSyncExternalStore(
		(listener) => {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		getSession,
		getSession
	);
}

export function hydrateSession(defaultUrl: string): void {
	if (typeof window === 'undefined') {
		state = { ...state, gatewayUrl: defaultUrl.replace(/\/$/, ''), hydrated: true };
		return;
	}

	state = {
		gatewayUrl: (window.localStorage.getItem(URL_KEY) ?? defaultUrl).replace(/\/$/, ''),
		adminToken: window.localStorage.getItem(TOKEN_KEY) ?? '',
		hydrated: true
	};
	notify();
}

export function persistSession(): void {
	if (typeof window === 'undefined') return;
	window.localStorage.setItem(URL_KEY, state.gatewayUrl.replace(/\/$/, ''));
	window.localStorage.setItem(TOKEN_KEY, state.adminToken);
}

export function clearSession(): void {
	updateSession({ adminToken: '' });
	persistSession();
}

export function isAuthed(): boolean {
	return state.adminToken.trim().length > 0;
}
