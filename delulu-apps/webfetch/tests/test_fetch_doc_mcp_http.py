#!/usr/bin/env python3
"""MCP HTTP transport integration test for fetch_doc using the real MCP SDK."""

import asyncio
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

logger = logging.getLogger(__name__)


def find_free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def kill_server_process(child):
    if sys.platform != "win32":
        try:
            os.killpg(os.getpgid(child.pid), signal.SIGTERM)
        except (ProcessLookupError, OSError):
            pass
    child.kill()
    child.wait()


async def main():
    binary = sys.argv[1]
    fixture_url = sys.argv[2]
    port = find_free_port()

    child = subprocess.Popen(
        # --expose-local-networks: the fetch_doc e2e fetches a PDF from the
        # local fixture server (127.0.0.1), which the SSRF guard blocks by default.
        [binary, "--expose-local-networks", "http", "--port", str(port)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )

    server_url = f"http://127.0.0.1:{port}/mcp"

    try:
        # Wait for server
        start = time.time()
        while time.time() - start < 10:
            try:
                s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                s.settimeout(0.1)
                s.connect(("127.0.0.1", port))
                s.close()
                break
            except Exception:
                time.sleep(0.1)
        else:
            raise RuntimeError(f"MCP server not ready on port {port}")

        async with streamable_http_client(server_url) as (read, write, get_session_id):
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

    finally:
        kill_server_process(child)


if __name__ == "__main__":
    logging.basicConfig(level=logging.WARNING)
    asyncio.run(main())
