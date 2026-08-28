import prettier from 'eslint-config-prettier';
import js from '@eslint/js';
import astro from 'eslint-plugin-astro';
import globals from 'globals';
import ts from 'typescript-eslint';

export default [
	{
		ignores: ['dist/**', '.astro/**', '.vercel/**', '.output/**', 'node_modules/**']
	},
	js.configs.recommended,
	...ts.configs.recommended,
	...astro.configs.recommended,
	prettier,
	{
		languageOptions: { globals: { ...globals.browser, ...globals.node } },
		rules: {
			'no-undef': 'off'
		}
	}
];
