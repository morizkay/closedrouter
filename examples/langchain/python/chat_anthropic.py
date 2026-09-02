"""ChatAnthropic pointed at a local ClosedRouter gateway."""

from __future__ import annotations

import os

from langchain_anthropic import ChatAnthropic


def main() -> None:
    # Anthropic SDK wants the origin, not /v1
    base = os.environ.get("CLOSEDROUTER_ANTHROPIC_BASE", "http://localhost:8080")
    key = os.environ.get("CLOSEDROUTER_API_KEY", "")
    model = os.environ.get("CLOSEDROUTER_MODEL", "llama3")
    if not key:
        raise SystemExit("Set CLOSEDROUTER_API_KEY to a sk-cr-… key from the dashboard")

    llm = ChatAnthropic(
        base_url=base,
        api_key=key,
        model=model,
        default_headers={"anthropic-version": "2023-06-01"},
    )
    reply = llm.invoke("Say hello from ClosedRouter in one sentence.")
    print(reply.content)


if __name__ == "__main__":
    main()
