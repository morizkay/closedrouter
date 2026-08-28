import { session } from '$lib/session.svelte';
import type { GatewayError } from '$lib/types';

export class ApiClientError extends Error {
	status: number;

	constructor(message: string, status: number) {
		super(message);
		this.name = 'ApiClientError';
		this.status = status;
	}
}

export async function adminRequest<T>(path: string, init: RequestInit = {}): Promise<T> {
	const headers = new Headers(init.headers);
	headers.set('Authorization', `Bearer ${session.adminToken}`);
	if (init.body && !headers.has('Content-Type')) {
		headers.set('Content-Type', 'application/json');
	}

	const response = await fetch(`${session.gatewayUrl}${path}`, {
		...init,
		headers
	});

	if (response.status === 204) {
		return undefined as T;
	}

	const text = await response.text();
	let parsed: unknown = null;
	if (text) {
		try {
			parsed = JSON.parse(text) as unknown;
		} catch {
			parsed = { error: { message: text, type: 'parse_error' } };
		}
	}

	if (!response.ok) {
		const err = parsed as GatewayError | null;
		throw new ApiClientError(err?.error?.message ?? `Request failed (${response.status})`, response.status);
	}

	return parsed as T;
}

export async function chatCompletions(args: {
	apiKey: string;
	model: string;
	messages: Array<{ role: string; content: string }>;
	stream: boolean;
}): Promise<Response> {
	return fetch(`${session.gatewayUrl}/v1/chat/completions`, {
		method: 'POST',
		headers: {
			Authorization: `Bearer ${args.apiKey}`,
			'Content-Type': 'application/json'
		},
		body: JSON.stringify({
			model: args.model,
			messages: args.messages,
			stream: args.stream
		})
	});
}
