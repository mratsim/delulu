#!/usr/bin/env python3
"""MCP stdio integration tests for paper-search-pubmed."""

import asyncio
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.resolve()))

from mcp.client.stdio import stdio_client, StdioServerParameters
from mcp import ClientSession

from paper_search_mcp_test_utils import find_server_binary


async def test_initialize(session):
    result = await asyncio.wait_for(session.initialize(), timeout=5)
    assert result.protocolVersion == "2025-03-26"
    return True

async def test_list_tools(session):
    tools = await asyncio.wait_for(session.list_tools(), timeout=5)
    names = [t.name for t in tools.tools]
    print(f"  Tools: {names}")
    assert "search_pubmed" in names
    assert "get_summaries" in names
    assert "fetch_abstracts" in names
    assert "find_related" in names
    assert "get_database_info" in names
    assert "match_citation" in names
    return True

async def test_search_pubmed(session):
    print("\nTesting call_tool(search_pubmed)...")
    result = await asyncio.wait_for(
        session.call_tool("search_pubmed", {"query": "virtual reality anesthesiology"}),
        timeout=15,
    )
    assert len(result.content) > 0, "Response should have content"
    text = result.content[0].text if hasattr(result.content[0], "text") else str(result.content[0])
    assert "pmids" in text.lower() or "count" in text.lower(), (
        f"Response should contain search results, got: {text[:200]}"
    )
    print("  search_pubmed returned results successfully")
    return True

async def test_get_database_info(session):
    print("\nTesting call_tool(get_database_info)...")
    result = await asyncio.wait_for(
        session.call_tool("get_database_info"),
        timeout=15,
    )
    assert len(result.content) > 0, "Response should have content"
    print("  get_database_info returned results successfully")
    return True

async def run_all():
    binary = find_server_binary("delulu-pubmed-mcp")
    params = StdioServerParameters(command=str(binary), args=["stdio"])
    async with stdio_client(params) as (r, w):
        async with ClientSession(r, w) as session:
            tests = [test_initialize, test_list_tools, test_search_pubmed, test_get_database_info]
            for test in tests:
                try:
                    await test(session)
                    print(f"  ✅ {test.__name__}")
                except Exception as e:
                    print(f"  ❌ {test.__name__}: {e}")
                    raise
    print("\n✅ All PubMed stdio MCP tests passed!")

if __name__ == "__main__":
    asyncio.run(run_all())
