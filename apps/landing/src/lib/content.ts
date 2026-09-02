export const TAB_IDS = ['openai', 'anthropic', 'deepseek', 'glm', 'cursor'] as const;

export type TabId = (typeof TAB_IDS)[number];

export type SdkTab = {
	id: TabId;
	label: string;
};

export type ValueProp = {
	k: string;
	t: string;
	d: string;
};

export type HowStep = {
	n: string;
	t: string;
	d: string;
};

export const PAGE_TITLE = 'ClosedRouter — OpenRouter, but local';

export const PAGE_DESCRIPTION =
	'Self-hosted LLM gateway with OpenAI, Anthropic, DeepSeek, GLM, and Cursor-compatible APIs. Your models, your keys, your network.';

export const OG_DESCRIPTION =
	'An LLM API gateway you run yourself. Docker, privacy, OpenAI + Anthropic + DeepSeek + GLM + Cursor.';

export const SDK_TABS: readonly SdkTab[] = [
	{ id: 'openai', label: 'OpenAI SDK' },
	{ id: 'anthropic', label: 'Anthropic SDK' },
	{ id: 'deepseek', label: 'DeepSeek' },
	{ id: 'glm', label: 'GLM' },
	{ id: 'cursor', label: 'Cursor' }
];

export const SNIPPETS: Record<TabId, string> = {
	openai: `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:8080/v1",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

const res = await client.chat.completions.create({
  model: "llama3",
  messages: [{ role: "user", content: "hello" }],
});`,
	anthropic: `import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "http://localhost:8080",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

const res = await client.messages.create({
  model: "llama3",
  max_tokens: 256,
  messages: [{ role: "user", content: "hello" }],
});`,
	deepseek: `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:8080",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

const res = await client.chat.completions.create({
  model: "deepseek-chat",
  messages: [{ role: "user", content: "hello" }],
});`,
	glm: `import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://localhost:8080/v4",
  apiKey: process.env.CLOSEDROUTER_API_KEY,
});

await client.chat.completions.create({
  model: "glm-4.5",
  messages: [{ role: "user", content: "hello" }],
  thinking: { type: "enabled" },
});`,
	cursor: `Cursor Settings → Models
OpenAI Base URL: http://localhost:8080/v1
OpenAI API Key:  sk-cr-your-closedrouter-key
Model:           llama3`
};

export const CURL_SNIPPET = `curl http://localhost:8080/v1/chat/completions \\
  -H "Authorization: Bearer sk-cr-..." \\
  -H "Content-Type: application/json" \\
  -d '{"model":"llama3","messages":[{"role":"user","content":"hello"}]}'`;

export const DOCKER_SNIPPET = `git clone https://github.com/morizkay/closedrouter.git
cd closedrouter
cp .env.example .env
docker compose up --build          # slim: postgres + gateway + ui
docker compose --profile full up   # + traefik grafana loki prometheus langfuse`;

export const VALUE_PROPS: readonly ValueProp[] = [
	{
		k: '01',
		t: 'OpenAI + Cursor',
		d: 'Drop-in /v1/chat/completions and /v1/models. SSE, usage, tools.'
	},
	{
		k: '02',
		t: 'Anthropic, DeepSeek, GLM',
		d: '/v1/messages, DeepSeek reasoning_content, GLM thinking / glm-4.5.'
	},
	{
		k: '03',
		t: 'Self-hosted',
		d: 'Compose slim or full (Traefik, Grafana, Loki, Prometheus, Langfuse). Postgres + pgvector.'
	},
	{
		k: '04',
		t: 'Your models',
		d: 'Map llama3 or glm-4 to Ollama, vLLM, Groq, OpenAI, Anthropic, DeepSeek, Zhipu.'
	},
	{
		k: '05',
		t: 'Hermes memory',
		d: 'Store and recall embeddings per API key. 1536-d pgvector, OpenAI-compat embedders.'
	},
	{
		k: '06',
		t: 'LangChain',
		d: 'ChatOpenAI and ChatAnthropic pointed at ClosedRouter. Examples in the repo.'
	}
];

export const HOW_STEPS: readonly HowStep[] = [
	{
		n: '1',
		t: 'Issue keys',
		d: 'The dashboard mints ClosedRouter API keys. Clients never see upstream credentials.'
	},
	{
		n: '2',
		t: 'Map models',
		d: 'Friendly IDs route to an OpenAI-compat or Anthropic-compat base URL and upstream model name.'
	},
	{
		n: '3',
		t: 'Translate & proxy',
		d: 'OpenAI, Anthropic, DeepSeek, and GLM are converted when the upstream speaks another dialect, including streams.'
	}
];
