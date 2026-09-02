# LangChain → ClosedRouter

Python (and optional TypeScript) examples that point LangChain chat models at the gateway.

## Python

```bash
cd examples/langchain/python
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
export CLOSEDROUTER_API_KEY=sk-cr-...
export CLOSEDROUTER_MODEL=llama3
python chat_openai.py
python chat_anthropic.py
```

`ChatOpenAI` uses `CLOSEDROUTER_BASE_URL` (default `http://localhost:8080/v1`).  
`ChatAnthropic` uses `CLOSEDROUTER_ANTHROPIC_BASE` (default `http://localhost:8080`).

## TypeScript

```bash
cd examples/langchain/ts
npm install
CLOSEDROUTER_API_KEY=sk-cr-... npx tsx index.ts
```
