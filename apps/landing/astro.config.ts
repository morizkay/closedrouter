import tailwindcss from '@tailwindcss/vite';
import vercel from '@astrojs/vercel';
import { defineConfig } from 'astro/config';

/**
 * ClosedRouter marketing site.
 *
 * Deploy on Vercel with Root Directory `apps/landing`.
 * Static HTML at build time; `@astrojs/vercel` emits the Vercel Build Output API
 * so the project is detected as Astro rather than a generic static site.
 */
export default defineConfig({
	output: 'static',
	adapter: vercel(),
	vite: {
		plugins: [tailwindcss()]
	}
});
