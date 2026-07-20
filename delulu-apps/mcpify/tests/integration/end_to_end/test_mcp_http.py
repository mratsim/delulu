#!/usr/bin/env python3
"""MCP HTTP transport integration tests using the MCP Python SDK.

The Rust test harness starts the backend services and mcpify instances,
then invokes this script with the two mcpify ports as arguments.
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp.client.session import ClientSession
from mcp.client.streamable_http import streamable_http_client
from mcp_e2e_utils import kill_process


DEAD_SPEC = {
    "openapi": "3.0.0",
    "info": {"title": "Dead", "version": "1.0"},
    "paths": {
        "/fail": {
            "get": {
                "operationId": "willFail",
                "parameters": [],
            }
        }
    },
    "servers": [{"url": "http://127.0.0.1:1"}],
}


async def run_tests(binary_path: str, port_a: int, port_b: int) -> int:
    """Run all HTTP transport tests against two mcpify instances."""
    print(f"Service A: http://127.0.0.1:{port_a}/mcp")
    print(f"Service B: http://127.0.0.1:{port_b}/mcp")

    tests_passed = 0
    tests_total = 8

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

                    assert "data" in response_data, f"{label}: Response missing 'data' field: {response_data}"
                    assert response_data.get("success") is True, f"{label}: Response success not True: {response_data}"
                    assert response_data["data"] == data, (
                        f"{label}: Data mismatch\n  want: {data}\n  got:  {response_data['data']}"
                    )
                    print(f"  {label}: {tool} returned expected data ({len(response_data['data'])} items)")
                    tests_passed += 1

        except Exception as e:
            print(f"  {label}: FAILED — {e}")

    # Test upstream failure: mcpify proxies to a dead port
    print("  Upstream failure: testing error on dead backend...")
    binary = str(binary_path)
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump(DEAD_SPEC, f)
        spec_path = f.name

    dead_port = 9877
    child = subprocess.Popen(
        [binary, "http", "--host", "127.0.0.1", "--port", str(dead_port), spec_path],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        import socket as sock
        import time
        start = time.time()
        while time.time() - start < 5:
            try:
                s = sock.socket(sock.AF_INET, sock.SOCK_STREAM)
                s.settimeout(0.2)
                s.connect(("127.0.0.1", dead_port))
                s.close()
                break
            except (ConnectionRefusedError, OSError):
                time.sleep(0.1)

        url = f"http://127.0.0.1:{dead_port}/mcp"
        async with streamable_http_client(url) as (read_stream, write_stream, _):
            async with ClientSession(read_stream, write_stream) as session:
                await session.initialize()
                tools = await session.list_tools()
                assert len(tools.tools) == 1, "dead spec should register 1 tool"
                assert tools.tools[0].name == "willFail"
                tests_passed += 1  # list_tools works even if backend is down

                is_error = False
                try:
                    call_result = await session.call_tool("willFail", {})
                    is_error = getattr(call_result, 'isError', False)
                except Exception:
                    # SDK raises on MCP error response
                    is_error = True
                assert is_error, "call_tool should have failed (backend is dead)"
                print("  Upstream failure: got MCP error as expected")
                tests_passed += 1

    except Exception as e:
        print(f"  Upstream failure: FAILED — {e}")
    finally:
        kill_process(child)
        os.unlink(spec_path)

    print(f"\nResults: {tests_passed}/{tests_total} passed")
    return 0 if tests_passed == tests_total else 1


if __name__ == "__main__":
    if len(sys.argv) < 4:
        print(f"Usage: {sys.argv[0]} <binary_path> <port_a> <port_b>", file=sys.stderr)
        sys.exit(1)

    import asyncio

    binary = sys.argv[1]
    port_a = int(sys.argv[2])
    port_b = int(sys.argv[3])
    sys.exit(asyncio.run(run_tests(binary, port_a, port_b)))
