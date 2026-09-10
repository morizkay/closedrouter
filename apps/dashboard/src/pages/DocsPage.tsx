import { useState } from 'react';
import { Link } from '@tanstack/react-router';
import { getSession } from '../lib/session';

const snippetData = [
	{
		id: 'openai',
		title: 'OpenAI SDK',
		description: 'Drop-in compatibility for chat completions, tools, streaming, and usage.',
		code: (url: string) =>
			`import OpenAI from 'openai';\n\nconst client = new OpenAI({\n  baseURL: '${url}/v1',\n  apiKey: process.env.CLOSEDROUTER_KEY,\n});`
	},
	{
		id: 'anthropic',
		title: 'Anthropic SDK',
		description: 'Keep your existing Messages API client and point it at ClosedRouter.',
		code: (url: string) =>
			`import Anthropic from '@anthropic-ai/sdk';\n\nconst client = new Anthropic({\n  baseURL: '${url}',\n  apiKey: process.env.CLOSEDROUTER_KEY,\n});`
	},
	{
		id: 'deepseek',
		title: 'DeepSeek',
		description:
			'Use the OpenAI SDK with DeepSeek reasoning content forwarded through the gateway.',
		code: (url: string) =>
			`import OpenAI from 'openai';\n\nconst client = new OpenAI({\n  baseURL: '${url}',\n  apiKey: process.env.CLOSEDROUTER_KEY,\n});\n\nawait client.chat.completions.create({\n  model: 'deepseek-chat',\n  messages: [{ role: 'user', content: 'hello' }],\n});`
	},
	{
		id: 'glm',
		title: 'GLM / Zhipu',
		description: 'Forward GLM thinking controls and provider-specific request metadata.',
		code: (url: string) =>
			`import OpenAI from 'openai';\n\nconst client = new OpenAI({\n  baseURL: '${url}/v4',\n  apiKey: process.env.CLOSEDROUTER_KEY,\n});\n\nawait client.chat.completions.create({\n  model: 'glm-4',\n  messages: [{ role: 'user', content: 'hello' }],\n  thinking: { type: 'enabled' },\n});`
	},
	{
		id: 'cursor',
		title: 'Cursor',
		description: 'Use a friendly model ID from your catalog with Cursor’s OpenAI override.',
		code: (url: string) =>
			`Cursor Settings → Models\n1. Toggle OpenAI API Key / override\n2. OpenAI Base URL: ${url}/v1\n3. OpenAI API Key: a ClosedRouter sk-cr-… key\n4. Model: a friendly ID from the Models page\n\nCursor calls GET /v1/models and POST /v1/chat/completions (SSE).`
	},
	{
		id: 'langchain',
		title: 'LangChain',
		description: 'Point ChatOpenAI at the same base URL in a few lines of Python.',
		code: (url: string) =>
			`from langchain_openai import ChatOpenAI\n\nllm = ChatOpenAI(\n    base_url='${url}/v1',\n    api_key='sk-cr-...',\n    model='llama3',\n)`
	}
];

export function DocsPage(): React.JSX.Element {
	const { gatewayUrl } = getSession();
	const [copied, setCopied] = useState('');
	async function copy(id: string, text: string): Promise<void> {
		await navigator.clipboard.writeText(text);
		setCopied(id);
		window.setTimeout(() => setCopied((current) => (current === id ? '' : current)), 1800);
	}
	return (
		<div className="page-wrap">
			<div className="page-header">
				<div>
					<p className="eyebrow">Connect</p>
					<h1 className="page-title">Client setup</h1>
					<p className="page-description">
						Copy a starter config for your client. Your gateway URL is inserted automatically so
						every example is ready to use.
					</p>
				</div>
				<Link to="/keys" className="button-primary">
					Create an API key
				</Link>
			</div>
			<div className="steps-grid page-section">
				{[
					'Create a key|Issue a client credential in Access.',
					'Choose a client|Use one of the adapters below.',
					'Send a request|Your friendly model ID does the routing.'
				].map((step, index) => {
					const [title, copyText] = step.split('|');
					return (
						<div className="surface-subtle step-card" key={title}>
							<p className="mono-label">0{index + 1}</p>
							<strong>{title}</strong>
							<p>{copyText}</p>
						</div>
					);
				})}
			</div>
			<div className="docs-list page-section">
				{snippetData.map((snippet, index) => {
					const code = snippet.code(gatewayUrl);
					return (
						<section className="surface doc-card" key={snippet.id}>
							<div className="doc-header">
								<div className="doc-title">
									<span className="doc-index">0{index + 1}</span>
									<div>
										<h2>{snippet.title}</h2>
										<p>{snippet.description}</p>
									</div>
								</div>
								<button
									type="button"
									className="button-secondary"
									onClick={() => void copy(snippet.id, code)}
								>
									{copied === snippet.id ? 'Copied' : 'Copy code'}
								</button>
							</div>
							<pre className="doc-code">{code}</pre>
						</section>
					);
				})}
			</div>
			<section className="surface card-padding page-section">
				<div className="memory-heading">
					<span className="badge purple">HERMES</span>
					<div>
						<h2>Memory API</h2>
						<p>
							Store and recall embeddings per API key. Configure a model with capability{' '}
							<code>embedding</code> on the Models page.
						</p>
					</div>
				</div>
				<pre className="doc-code inset-code">{`POST ${gatewayUrl}/v1/hermes/memories\nAuthorization: Bearer sk-cr-…\n{\n  "content": "user prefers terse answers",\n  "agent": "hermes",\n  "embed": true\n}`}</pre>
				<p className="docs-note">
					See <code>docs/HERMES.md</code> in the repository for the full request and search schema.
				</p>
			</section>
		</div>
	);
}
