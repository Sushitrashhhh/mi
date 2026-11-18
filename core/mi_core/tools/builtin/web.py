"""Web tools: search (via Gemini grounding) and fetch page as text. Both taint the context."""

from __future__ import annotations

from mi_core.tools.registry import Tier, ToolResult, tool


@tool(
    name="web.search",
    tier=Tier.READ,
    description="Search the web using Gemini's grounded search. Results are untrusted.",
    summary="Search: {query}",
)
async def web_search(query: str) -> ToolResult:
    return ToolResult(
        content=f"[web.search is a placeholder in Phase 1. Query: {query}] "
        "Grounded search will be wired to Gemini's google_search tool in a later phase."
    )


@tool(
    name="web.fetch",
    tier=Tier.READ,
    description="Fetch a web page and return its text content. Content is untrusted.",
    summary="Fetch {url}",
)
async def web_fetch(url: str) -> ToolResult:
    return ToolResult(
        content=f"[web.fetch is a placeholder in Phase 1. URL: {url}] "
        "Full page fetching will be implemented in a later phase."
    )
