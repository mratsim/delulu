#!/usr/bin/env python3
"""MCP HTTP end-to-end test for paper-search-arxiv using a fixture server.

The Rust test harness starts a fixture server + the MCP server in HTTP mode,
then invokes this script with:
    <mcp_port> <config_path>
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp.client.session import ClientSession
from mcp.client.streamable_http import streamable_http_client


async def run_tests(mcp_port: int, config_path: str) -> int:
    with open(config_path) as f:
        config = json.load(f)
    fixture_url = config["fixture_url"]
    server_url = f"http://127.0.0.1:{mcp_port}/mcp"

    print(f"MCP server: {server_url}")
    print(f"Fixture server: {fixture_url}")

    tests_passed = 0
    tests_total = 3

    try:
        async with streamable_http_client(server_url) as (read, write, get_session_id):
            async with ClientSession(read, write) as session:
                # Test initialize
                result = await session.initialize()
                assert result.protocolVersion == "2025-03-26"
                print(f"  Initialized (protocol {result.protocolVersion})")
                tests_passed += 1

                # Test list_tools
                tools = await session.list_tools()
                names = [t.name for t in tools.tools]
                assert "search_papers" in names
                assert "get_papers_by_id" in names
                print(f"  Found {len(tools.tools)} tools: {names}")
                tests_passed += 1

                # Test call_tool(search_papers)
                result = await session.call_tool("search_papers", {"query": "all:electron"})
                assert len(result.content) > 0
                text = result.content[0].text
                data = json.loads(text)
                assert len(data) > 0
                assert "title" in data[0]
                print(f"  search_papers returned {len(data)} papers")
                tests_passed += 1

    except Exception as e:
        print(f"  FAILED — {e}")
        return 1

    print(f"\nResults: {tests_passed}/{tests_total} passed")
    return 0 if tests_passed == tests_total else 1


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <mcp_port> <config_path>", file=sys.stderr)
        sys.exit(1)

    import asyncio
    mcp_port = int(sys.argv[1])
    config_path = sys.argv[2]
    sys.exit(asyncio.run(run_tests(mcp_port, config_path)))
