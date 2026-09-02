<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import favicon from '$lib/assets/favicon.svg';
	import { clearSession } from '$lib/session.svelte';

	let { children }: { children: import('svelte').Snippet } = $props();

	const links = [
		{ href: '/' as const, label: 'Overview' },
		{ href: '/keys' as const, label: 'API keys' },
		{ href: '/models' as const, label: 'Models' },
		{ href: '/playground' as const, label: 'Playground' },
		{ href: '/docs' as const, label: 'Docs' }
	];

	function signOut(): void {
		clearSession();
		void goto(resolve('/'));
	}
</script>

<div class="flex min-h-screen">
	<aside class="sticky top-0 flex h-screen w-56 shrink-0 flex-col border-r border-line bg-panel/60">
		<a href={resolve('/')} class="flex items-center gap-2 px-5 py-6">
			<img src={favicon} alt="ClosedRouter" class="h-7 w-7" />
			<span class="text-xs font-semibold tracking-[0.18em] uppercase">ClosedRouter</span>
		</a>
		<nav class="flex flex-1 flex-col gap-0.5 px-3">
			{#each links as link (link.href)}
				<a
					href={resolve(link.href)}
					class="rounded-lg px-3 py-2 text-sm {page.url.pathname === link.href
						? 'bg-white/10 text-paper'
						: 'text-mist hover:bg-white/5 hover:text-paper'}"
				>
					{link.label}
				</a>
			{/each}
		</nav>
		<button
			type="button"
			class="mx-3 mb-5 rounded-lg px-3 py-2 text-left text-sm text-mist hover:text-paper"
			onclick={signOut}
		>
			Disconnect
		</button>
	</aside>
	<main class="min-w-0 flex-1 px-8 py-8">{@render children()}</main>
</div>
