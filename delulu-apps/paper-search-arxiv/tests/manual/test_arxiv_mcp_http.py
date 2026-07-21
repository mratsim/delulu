#!/usr/bin/env python3
"""MCP HTTP transport integration tests for paper-search-arxiv."""

import asyncio
import subprocess
import sys
from pathlib import Path

import os
sys.path.insert(0, str(Path(__file__).parent.resolve()))

from mcp.client.session import ClientSession
from mcp.client.streamable_http import streamable_http_client

from paper_search_mcp_test_utils import find_server_binary, wait_for_server


def kill_server(child: subprocess.Popen) -> None:
    import os, signal
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
    print("Testing MCP initialization...")
    result = await asyncio.wait_for(session.initialize(), timeout=5)
    print(f"  Initialized with protocol version: {result.protocolVersion}")
    assert result.protocolVersion == "2025-03-26"
    return True


async def test_list_tools(session) -> bool:
    print("Testing list_tools...")
    tools = await asyncio.wait_for(session.list_tools(), timeout=5)
    tool_names = [t.name for t in tools.tools]
    print(f"  Found {len(tools.tools)} tools: {tool_names}")
    assert "search_papers" in tool_names
    assert "get_papers_by_id" in tool_names
    return True


async def run_all_tests():
    server_binary = find_server_binary("delulu-arxiv-mcp")
    print(f"Using server binary: {server_binary}")

    port = 9876
    server_url = f"http://127.0.0.1:{port}/mcp"

    child = subprocess.Popen(
        [str(server_binary), "http", "--port", str(port)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        preexec_fn=lambda: None if sys.platform == "win32" else os.setpgrp(),
    )

    try:
        wait_for_server(port)
        print(f"Server ready on port {port}")

        async with streamable_http_client(server_url) as (
            read_stream, write_stream, get_session_id,
        ):
            async with ClientSession(read_stream, write_stream) as session:
                tests = [test_mcp_initialize, test_list_tools]
                for test in tests:
                    try:
                        await test(session)
                        print(f"  ✅ {test.__name__}")
                    except Exception as e:
                        print(f"  ❌ {test.__name__}: {e}")
                        raise
        print("\n✅ All HTTP MCP tests passed!")
    finally:
        kill_server(child)


if __name__ == "__main__":
    asyncio.run(run_all_tests())
