import path from 'node:path';
import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import { defineConfig, includeIgnoreFile } from 'eslint/config';
import globals from 'globals';
import ts from 'typescript-eslint';

const gitignorePath = path.resolve(import.meta.dirname, '.gitignore');

export default defineConfig(
	includeIgnoreFile(gitignorePath),
	js.configs.recommended,
	ts.configs.recommended,
	{
		files: ['**/*.{js,mjs,ts,tsx}'],
		languageOptions: {
			globals: { ...globals.browser, ...globals.node }
		}
	},
	prettier
);
