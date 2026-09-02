import { ChatOpenAI } from "@langchain/openai";
import { ChatAnthropic } from "@langchain/anthropic";

const key = process.env.CLOSEDROUTER_API_KEY;
if (!key) {
  throw new Error("Set CLOSEDROUTER_API_KEY");
}

const model = process.env.CLOSEDROUTER_MODEL ?? "llama3";

const openai = new ChatOpenAI({
  apiKey: key,
  model,
  configuration: {
    baseURL: process.env.CLOSEDROUTER_BASE_URL ?? "http://localhost:8080/v1",
  },
});

const anthropic = new ChatAnthropic({
  apiKey: key,
  model,
  clientOptions: {
    baseURL: process.env.CLOSEDROUTER_ANTHROPIC_BASE ?? "http://localhost:8080",
  },
});

const which = process.argv[2] ?? "openai";
const llm = which === "anthropic" ? anthropic : openai;
const res = await llm.invoke("Say hello from ClosedRouter in one sentence.");
const text =
  typeof res.content === "string"
    ? res.content
    : JSON.stringify(res.content);
console.log(text);
