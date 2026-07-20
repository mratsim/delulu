#!/usr/bin/env python3
"""MCP HTTP transport integration tests for webfetch using Python MCP SDK."""

import asyncio
import json
import logging
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp.client.session import ClientSession
from mcp.client.streamable_http import streamable_http_client

from webfetch_test_utils import (
    find_server_binary,
    wait_for_server,
)


logger = logging.getLogger(__name__)


def kill_server_process(child: subprocess.Popen) -> None:
    """Kill server process and all its children."""
    if sys.platform != "win32":
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except (ProcessLookupError, OSError):
            pass
    child.terminate()
    try:
        child.wait(timeout=2)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait()


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
    assert "webfetch" in tool_names, "webfetch tool should be available"
    assert "webfetch_raw" in tool_names, "webfetch_raw tool should be available"
    return True


async def test_call_webfetch(session) -> bool:
    """Test calling webfetch with a valid URL."""
    print("\nTesting call_tool(webfetch)...")

    url = "https://example.com"
    print(f"  Query: {url}")

    result = await asyncio.wait_for(
        session.call_tool("webfetch", {"url": url}),
        timeout=10,
    )

    content = result.content
    assert len(content) > 0, "Response should have content"
    assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"

    text = content[0].text
    print(f"  Response length: {len(text)} chars")

    # webfetch returns markdown with YAML frontmatter
    assert text.startswith("---"), f"Expected YAML frontmatter, got: {text[:80]}"
    assert "---" in text[3:], "Expected closing frontmatter delimiter"

    print("  Response validated (contains YAML frontmatter)")
    return True


async def test_call_webfetch_raw(session) -> bool:
    """Test calling webfetch_raw with a valid URL."""
    print("\nTesting call_tool(webfetch_raw)...")

    url = "https://example.com"
    print(f"  Query: {url}")

    result = await asyncio.wait_for(
        session.call_tool("webfetch_raw", {"url": url}),
        timeout=10,
    )

    content = result.content
    assert len(content) > 0, "Response should have content"
    assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"

    text = content[0].text
    print(f"  Response length: {len(text)} chars")

    # webfetch_raw returns JSON
    try:
        data = json.loads(text)
        print("  Got valid JSON response")
        # Should have GenericHtml (or other ExtractionResult variant)
        assert any(k in data for k in ("GenericHtml", "Reddit", "Discourse")), (
            f"Response should contain expected variant key, got: {list(data.keys())[:5]}"
        )
        print(f"  Response validated: keys={list(data.keys())[:6]}, nested={list(data.get('GenericHtml', {}).keys())[:3]}")
    except (json.JSONDecodeError, ValueError) as e:
        print(f"  Response is not valid JSON: {e}")
        print(f"     ====\n    {text[:500]}\n====\n")
        return False

    return True


async def test_call_webfetch_invalid_url(session) -> bool:
    """Test calling webfetch with an invalid URL."""
    print("\nTesting call_tool(webfetch) with invalid URL...")

    result = await asyncio.wait_for(
        session.call_tool("webfetch", {"url": "ftp://invalid.example.com"}),
        timeout=10,
    )

    content = result.content
    assert len(content) > 0, "Response should have content"
    text = content[0].text if hasattr(content[0], "text") else str(content[0])
    print(f"  Got error response: {text[:100]}")

    # Server returns a structured error in markdown frontmatter
    assert "error" in text.lower() or "fetch failed" in text.lower(), (
        f"Expected error response, got: {text[:200]}"
    )
    print("  Error response validated")
    return True


def find_free_port() -> int:
    """Find a free TCP port on localhost."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


async def run_http_tests(port: int) -> int:
    """Run all webfetch MCP HTTP transport integration tests."""
    print("=" * 60)
    print("Webfetch MCP HTTP Transport Integration Tests")
    print("=" * 60)
    print(f"Using server binary: {find_server_binary()}")
    print(f"Target: http://127.0.0.1:{port}/mcp")
    print()

    binary = str(find_server_binary())
    child = subprocess.Popen(
        [binary, "http", "--port", str(port)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )

    url = f"http://127.0.0.1:{port}/mcp"

    try:
        wait_for_server(port)

        async with streamable_http_client(url) as (
            read_stream,
            write_stream,
            get_session_id,
        ):
            async with ClientSession(read_stream, write_stream) as session:
                tests_passed = 0
                tests_total = 5

                if await test_mcp_initialize(session):
                    tests_passed += 1

                if await test_list_tools(session):
                    tests_passed += 1

                if await test_call_webfetch(session):
                    tests_passed += 1

                if await test_call_webfetch_raw(session):
                    tests_passed += 1

                if await test_call_webfetch_invalid_url(session):
                    tests_passed += 1

                print("\n" + "=" * 60)
                print(f"Tests: {tests_passed}/{tests_total} passed")
                print("=" * 60)

                return 0 if tests_passed == tests_total else 1

    except Exception as e:
        print(f"Error during tests: {e}")
        import traceback
        traceback.print_exc()
        return 1
    finally:
        child.terminate()
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
        # MCP client logs "Session termination failed" during its own cleanup
        # because we killed the server first. Expected and harmless.
        logging.getLogger("mcp.client.streamable_http").warning(
            "test_webfetch_http.py: Server killed before client cleanup - "
            "'Session termination failed' is expected"
        )


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else find_free_port()
    sys.exit(asyncio.run(run_http_tests(port)))
