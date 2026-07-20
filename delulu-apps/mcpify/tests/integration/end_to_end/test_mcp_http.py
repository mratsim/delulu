#!/usr/bin/env python3
"""MCP HTTP transport integration tests using the MCP Python SDK.

The Rust test harness starts the backend services and mcpify instances,
then invokes this script with the two mcpify ports as arguments.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp.client.session import ClientSession
from mcp.client.streamable_http import streamable_http_client


async def run_tests(port_a: int, port_b: int) -> int:
    """Run all HTTP transport tests against two mcpify instances."""
    print(f"Service A: http://127.0.0.1:{port_a}/mcp")
    print(f"Service B: http://127.0.0.1:{port_b}/mcp")

    tests_passed = 0
    tests_total = 6

    for label, port, tool, data in [
        ("Service A", port_a, "listUsers", [{"id": 1, "name": "Alice"}]),
        ("Service B", port_b, "listItems", [{"id": 1, "item": "Widget"}]),
    ]:
        url = f"http://127.0.0.1:{port}/mcp"
        try:
            async with streamable_http_client(url) as (read_stream, write_stream, _):
                async with ClientSession(read_stream, write_stream) as session:
                    # Test initialize
                    result = await session.initialize()
                    assert result.protocolVersion == "2025-03-26", (
                        f"{label}: Expected protocol 2025-03-26, got {result.protocolVersion}"
                    )
                    print(f"  {label}: Initialized (protocol {result.protocolVersion})")
                    tests_passed += 1

                    # Test list_tools
                    tools = await session.list_tools()
                    names = [t.name for t in tools.tools]
                    assert tool in names, (
                        f"{label}: Expected tool '{tool}' not found in {names}"
                    )
                    print(f"  {label}: Found {len(tools.tools)} tools: {names}")
                    tests_passed += 1

                    # Test call_tool
                    call_result = await session.call_tool(tool, {})
                    content = call_result.content
                    assert len(content) > 0, f"{label}: No content in response"
                    assert hasattr(content[0], "text"), f"{label}: Expected text content"

                    text = content[0].text
                    response_data = json.loads(text)

                    # The proxy wraps the backend response
                    assert "data" in response_data, f"{label}: Response missing 'data' field: {response_data}"
                    assert response_data.get("success") is True, f"{label}: Response success not True: {response_data}"
                    assert response_data["data"] == data, (
                        f"{label}: Data mismatch\n  want: {data}\n  got:  {response_data['data']}"
                    )
                    print(f"  {label}: {tool} returned expected data ({len(response_data['data'])} items)")
                    tests_passed += 1

        except Exception as e:
            print(f"  {label}: FAILED — {e}")

    print(f"\nResults: {tests_passed}/{tests_total} passed")
    return 0 if tests_passed == tests_total else 1


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <port_a> <port_b>", file=sys.stderr)
        sys.exit(1)

    import asyncio

    port_a = int(sys.argv[1])
    port_b = int(sys.argv[2])
    sys.exit(asyncio.run(run_tests(port_a, port_b)))
