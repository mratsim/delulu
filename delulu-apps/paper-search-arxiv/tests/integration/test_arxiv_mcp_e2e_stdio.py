#!/usr/bin/env python3
"""MCP stdio end-to-end test for paper-search-arxiv using a fixture server.

Follows the mcpify pattern: the Rust test harness starts a fixture server,
writes a config JSON file with the fixture URL, then invokes this script with:
    <mcp_binary_path> <config_path>
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp import ClientSession
from mcp.client.stdio import stdio_client, StdioServerParameters


async def run_tests(binary: str, fixture_url: str) -> int:
    print(f"Binary: {binary}")
    print(f"Fixture server: {fixture_url}")

    tests_passed = 0
    tests_total = 4

    server_params = StdioServerParameters(
        command=binary,
        args=["--api-base-url", fixture_url, "stdio"],
        env=None,
    )

    try:
        async with stdio_client(server_params) as (read, write):
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

                # Test call_tool(get_papers_by_id)
                result = await session.call_tool("get_papers_by_id", {"ids": "cond-mat/0011267"})
                assert len(result.content) > 0
                text = result.content[0].text
                data = json.loads(text)
                assert len(data) > 0
                print(f"  get_papers_by_id returned {len(data)} papers")
                tests_passed += 1

    except Exception as e:
        print(f"  FAILED — {e}")
        return 1

    print(f"\nResults: {tests_passed}/{tests_total} passed")
    return 0 if tests_passed == tests_total else 1


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <binary_path> <config_path>", file=sys.stderr)
        sys.exit(1)

    import asyncio
    binary = sys.argv[1]
    config_path = sys.argv[2]
    with open(config_path) as f:
        config = json.load(f)
    fixture_url = config["fixture_url"]
    sys.exit(asyncio.run(run_tests(binary, fixture_url)))
