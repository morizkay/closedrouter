<script lang="ts">
	import './layout.css';
	import { env } from '$env/dynamic/public';
	import favicon from '$lib/assets/favicon.svg';
	import LoginPanel from '$lib/LoginPanel.svelte';
	import Shell from '$lib/Shell.svelte';
	import { hydrateSession, isAuthed, session } from '$lib/session.svelte';

	let { children }: { children: import('svelte').Snippet } = $props();

	hydrateSession(env.PUBLIC_GATEWAY_URL ?? 'http://localhost:8080');
</script>

<svelte:head>
	<title>ClosedRouter</title>
	<link rel="icon" href={favicon} />
</svelte:head>

{#if !session.hydrated}
	<div class="p-10 text-sm text-mist">Loading…</div>
{:else if !isAuthed()}
	<LoginPanel />
{:else}
	<Shell>{@render children()}</Shell>
{/if}
