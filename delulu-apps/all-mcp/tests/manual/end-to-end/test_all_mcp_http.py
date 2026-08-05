#!/usr/bin/env python3
"""MCP HTTP transport integration tests for delulu-all-mcp.

Starts the MCP server in HTTP mode (with optional fixture base-url flags),
connects via streamable_http_client, and runs the shared suite (initialize,
list_tools, the three offline fixture-backed paper tools, and the error paths)
through the official MCP Python SDK.

Usage:
    python3 test_all_mcp_http.py <binary_path> [--arxiv-api-base-url URL --iacr-api-base-url URL --pubmed-api-base-url URL]

Exit codes:
    0 = all passed (or gracefully skipped with at least one test passing)
    1 = test failure or all tests skipped
    2 = environment error (missing SDK)
"""

import asyncio
import logging
import socket
import subprocess
import sys
from pathlib import Path

# MCP Python SDK imports (explicit paths)
try:
    from mcp import ClientSession
    from mcp.client.streamable_http import streamable_http_client
except ImportError:
    print("FAIL: mcp Python SDK not installed. Run: pip install 'mcp>=1.0.0'")
    sys.exit(2)

sys.path.insert(0, str(Path(__file__).parent))
from all_mcp_test_utils import (
    find_server_binary,
    print_results,
    run_all_mcp_tests,
    wait_for_server,
    kill_server_process,
    PROTOCOL_VERSION,
)


def find_free_port() -> int:
    """Find a free TCP port on localhost.

    Known limitation: TOCTOU race between bind+close and server bind.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


async def run_http_tests(binary: Path, port: int, base_url_flags: list) -> int:
    """Run all MCP HTTP transport tests.

    Manages the server subprocess lifecycle: spawn, wait, verify, test, kill.
    """
    binary_str = str(binary)
    child = subprocess.Popen(
        [binary_str] + base_url_flags + ["http", "--port", str(port)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )

    try:
        wait_for_server(port)
        print(f"  Server ready on http://127.0.0.1:{port}")

        url = f"http://127.0.0.1:{port}/mcp"
        async with streamable_http_client(url) as (
            read_stream,
            write_stream,
            _get_session_id,
        ):
            async with ClientSession(read_stream, write_stream) as session:
                results = await run_all_mcp_tests(session)

        return print_results(results)

    except Exception as e:
        print(f"Error during HTTP tests: {e}")
        import traceback
        traceback.print_exc()
        return 1
    finally:
        kill_server_process(child)
        # MCP client logs "Session termination failed" during its own cleanup
        # because we killed the server first. Expected and harmless.
        logging.getLogger("mcp.client.streamable_http").warning(
            "test_all_mcp_http.py: Server killed before client cleanup - "
            "'Session termination failed' is expected"
        )


async def main():
    """Main entry point with port conflict retry logic."""
    # Determine binary path
    if len(sys.argv) >= 2:
        binary = Path(sys.argv[1])
    else:
        binary = find_server_binary()
    base_url_flags = sys.argv[2:]

    print("=" * 60)
    print("delulu-all-mcp Integration Tests (HTTP)")
    print("=" * 60)
    print(f"Using server binary: {binary}")
    print(f"Protocol version: {PROTOCOL_VERSION}")
    if base_url_flags:
        print(f"Fixture base-url flags: {base_url_flags}")
    print()

    # Port conflict retry: up to 3 attempts
    max_retries = 3
    exit_code = 1

    for attempt in range(1, max_retries + 1):
        port = find_free_port()
        if attempt > 1:
            print(f"\nRetry attempt {attempt}/{max_retries}...")
            print(f"  Trying port {port}")

        try:
            exit_code = await run_http_tests(binary, port, base_url_flags)
            if exit_code == 0:
                return 0
            print(f"  Attempt {attempt}/{max_retries}: HTTP tests returned exit code {exit_code}")
        except Exception as e:
            print(f"  Attempt {attempt}/{max_retries} failed: {e}")
            exit_code = 1

        # Retry logic: if not last attempt, prepare for retry
        if attempt < max_retries:
            print("  Retrying with new port...")

    # All retries exhausted
    print(f"All {max_retries} attempts failed")
    return exit_code


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
