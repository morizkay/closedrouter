"""ChatOpenAI pointed at a local ClosedRouter gateway."""

from __future__ import annotations

import os

from langchain_openai import ChatOpenAI


def main() -> None:
    base = os.environ.get("CLOSEDROUTER_BASE_URL", "http://localhost:8080/v1")
    key = os.environ.get("CLOSEDROUTER_API_KEY", "")
    model = os.environ.get("CLOSEDROUTER_MODEL", "llama3")
    if not key:
        raise SystemExit("Set CLOSEDROUTER_API_KEY to a sk-cr-… key from the dashboard")

    llm = ChatOpenAI(
        base_url=base,
        api_key=key,
        model=model,
        temperature=0.2,
    )
    reply = llm.invoke("Say hello from ClosedRouter in one sentence.")
    print(reply.content)


if __name__ == "__main__":
    main()
