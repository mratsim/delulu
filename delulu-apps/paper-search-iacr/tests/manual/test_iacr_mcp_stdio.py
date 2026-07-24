#!/usr/bin/env python3
"""MCP stdio integration tests for paper-search-iacr."""

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
    assert "list_recent_papers" in names
    assert "get_paper_details" in names
    assert "paper_pdf_url" in names
    return True

async def test_list_recent_papers(session):
    print("\nTesting call_tool(list_recent_papers)...")
    result = await asyncio.wait_for(
        session.call_tool("list_recent_papers", {}),
        timeout=15,
    )
    assert len(result.content) > 0, "Response should have content"
    text = result.content[0].text if hasattr(result.content[0], "text") else str(result.content[0])
    assert "title" in text.lower() or "paper" in text.lower() or len(text) > 50, (
        f"Response should contain paper data, got: {text[:200]}"
    )
    print("  list_recent_papers returned results successfully")
    return True

async def test_get_paper_details(session):
    print("\nTesting call_tool(get_paper_details)...")
    result = await asyncio.wait_for(
        session.call_tool("get_paper_details", {"year": 2025, "number": 1}),
        timeout=15,
    )
    assert len(result.content) > 0, "Response should have content"
    print("  get_paper_details returned results successfully")
    return True

async def run_all():
    binary = find_server_binary("delulu-iacr-mcp")
    params = StdioServerParameters(command=str(binary), args=["stdio"])
    async with stdio_client(params) as (r, w):
        async with ClientSession(r, w) as session:
            tests = [test_initialize, test_list_tools, test_list_recent_papers, test_get_paper_details]
            for test in tests:
                try:
                    await test(session)
                    print(f"  ✅ {test.__name__}")
                except Exception as e:
                    print(f"  ❌ {test.__name__}: {e}")
                    raise
    print("\n✅ All IACR stdio MCP tests passed!")

if __name__ == "__main__":
    asyncio.run(run_all())
