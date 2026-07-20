#!/usr/bin/env python3
"""MCP stdio transport integration tests using the MCP Python SDK.

The Rust test harness starts the backend services and writes spec files,
then invokes this script with the two spec file paths as arguments.
This script spawns mcpify instances via stdio_client for each spec.
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp import ClientSession
from mcp.client.stdio import stdio_client, StdioServerParameters


async def run_tests(binary: str, spec_a: str, spec_b: str) -> int:
    print(f"Binary: {binary}")
    print(f"Spec A: {spec_a}")
    print(f"Spec B: {spec_b}")

    tests_passed = 0
    tests_total = 6

    for label, spec, tool, data in [
        ("Service A", spec_a, "listUsers", [{"id": 1, "name": "Alice"}]),
        ("Service B", spec_b, "listItems", [{"id": 1, "item": "Widget"}]),
    ]:
        server_params = StdioServerParameters(
            command=binary,
            args=["stdio", spec],
            env=None,
        )
        try:
            async with stdio_client(server_params) as (read, write):
                async with ClientSession(read, write) as session:
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
    if len(sys.argv) < 4:
        print(f"Usage: {sys.argv[0]} <binary_path> <spec_a_path> <spec_b_path>", file=sys.stderr)
        sys.exit(1)

    import asyncio

    binary = sys.argv[1]
    spec_a = sys.argv[2]
    spec_b = sys.argv[3]
    sys.exit(asyncio.run(run_tests(binary, spec_a, spec_b)))
