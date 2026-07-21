#!/usr/bin/env python3
"""MCP server integration tests for paper-search-arxiv using stdio transport."""

import asyncio
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.resolve()))

from mcp.client.stdio import stdio_client, StdioServerParameters
from mcp import ClientSession

from paper_search_mcp_test_utils import find_server_binary


async def test_mcp_initialize(session) -> bool:
    """Test MCP initialization."""
    print("Testing MCP initialization...")
    result = await asyncio.wait_for(session.initialize(), timeout=5)
    print(f"  Initialized with protocol version: {result.protocolVersion}")
    assert result.protocolVersion == "2025-03-26", (
        f"Expected 2025-03-26, got {result.protocolVersion}"
    )
    return True


async def test_list_tools(session) -> bool:
    """Test listing available tools."""
    print("Testing list_tools...")
    tools = await asyncio.wait_for(session.list_tools(), timeout=5)
    tool_names = [t.name for t in tools.tools]
    print(f"  Found {len(tools.tools)} tools: {tool_names}")
    assert "search_papers" in tool_names, "search_papers tool should be available"
    assert "get_papers_by_id" in tool_names, "get_papers_by_id tool should be available"
    return True


async def test_call_search_papers(session) -> bool:
    """Test calling search_papers tool."""
    print("\nTesting call_tool(search_papers)...")
    result = await asyncio.wait_for(
        session.call_tool("search_papers", {"query": "transformer"}),
        timeout=15,
    )
    content = result.content
    assert len(content) > 0, "Response should have content"
    text = content[0].text if hasattr(content[0], "text") else str(content[0])
    assert "transformer" in text.lower() or "papers" in text.lower(), (
        f"Response should contain paper data, got: {text[:200]}"
    )
    print("  search_papers returned results successfully")
    return True


async def run_all_tests():
    """Run all stdio MCP tests."""
    server_binary = find_server_binary("delulu-arxiv-mcp")
    print(f"Using server binary: {server_binary}")
    print()

    server_params = StdioServerParameters(
        command=str(server_binary),
        args=["stdio"],
    )

    async with stdio_client(server_params) as (read, write):
        async with ClientSession(read, write) as session:
            tests = [
                test_mcp_initialize,
                test_list_tools,
                test_call_search_papers,
            ]
            for test in tests:
                try:
                    result = await test(session)
                    print(f"  ✅ {test.__name__}")
                except Exception as e:
                    print(f"  ❌ {test.__name__}: {e}")
                    raise
    print("\n✅ All stdio MCP tests passed!")


if __name__ == "__main__":
    asyncio.run(run_all_tests())
