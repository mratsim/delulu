#!/usr/bin/env python3
"""MCP stdio integration test for fetch_doc using the real MCP SDK."""

import asyncio
import logging
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp.client.stdio import stdio_client, StdioServerParameters
from mcp import ClientSession

logger = logging.getLogger(__name__)


async def main():
    binary = sys.argv[1]
    fixture_url = sys.argv[2]

    server_params = StdioServerParameters(
        command=binary,
        args=["stdio"],
    )

    async with stdio_client(server_params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()

            # Test 1: tools/list contains fetch_doc
            result = await session.list_tools()
            tool_names = [t.name for t in result.tools]
            assert "fetch_doc" in tool_names, f"fetch_doc not in tools: {tool_names}"
            print(f"OK: tools/list contains fetch_doc ({len(result.tools)} tools)")

            # Test 2: fetch_doc with a real PDF URL returns markdown
            result = await session.call_tool("fetch_doc", {"url": fixture_url})
            text = result.content[0].text
            assert "source_type: document" in text, (
                f"Expected document source_type, got: {text[:200]}"
            )
            assert len(text) > 100, f"Response too short: {len(text)} chars"
            print(f"OK: fetch_doc returned {len(text)} chars of markdown")


if __name__ == "__main__":
    logging.basicConfig(level=logging.WARNING)
    asyncio.run(main())
