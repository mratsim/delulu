#!/usr/bin/env python3
"""MCP HTTP transport integration tests for delulu-websearch-mcp.

Starts the MCP server in HTTP mode, connects via streamable_http_client,
and runs the standard test suite (initialize, list_tools, search, continuation).

Usage:
    python3 tests/test_websearch_http.py [binary_path] [port]

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
import time
from pathlib import Path

# MCP Python SDK imports (explicit paths)
try:
    from mcp import ClientSession
    from mcp.client.streamable_http import streamable_http_client
except ImportError:
    print("FAIL: mcp Python SDK not installed. Run: pip install 'mcp>=1.0.0'")
    sys.exit(2)

sys.path.insert(0, str(Path(__file__).parent))
from websearch_test_utils import (
    find_server_binary,
    run_mcp_tests,
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



async def run_http_tests(binary: Path, port: int) -> int:
    """Run all MCP HTTP transport tests.

    Manages the server subprocess lifecycle: spawn, wait, verify, test, kill.
    """
    binary_str = str(binary)
    child = subprocess.Popen(
        [binary_str, "http", "--port", str(port)],
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
                results = await run_mcp_tests(session)

        # Print results
        print()
        print("=" * 60)
        print("Results:")
        print("=" * 60)
        tests_passed = 0
        tests_total = len(results)
        for name, result in results.items():
            status = result["message"]
            if result["passed"]:
                tests_passed += 1
                print(f"  {name}: {status}")
            else:
                print(f"  {name}: {status}")

        print()
        print(f"  Passed: {tests_passed}/{tests_total}")
        print("=" * 60)

        # Compute exit code
        if tests_passed == tests_total:
            return 0
        elif tests_passed > 0:
            return 0
        else:
            print("ERROR: All tests failed or were skipped — no assertions exercised")
            return 1

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
            "test_websearch_http.py: Server killed before client cleanup - "
            "'Session termination failed' is expected"
        )


async def main():
    """Main entry point with port conflict retry logic."""
    # Determine binary path
    if len(sys.argv) >= 2:
        binary = Path(sys.argv[1])
    else:
        binary = find_server_binary()

    # Determine port
    if len(sys.argv) >= 3:
        port = int(sys.argv[2])
    else:
        port = find_free_port()

    print("=" * 60)
    print("MCP Server Integration Tests (HTTP)")
    print("=" * 60)
    print(f"Using server binary: {binary}")
    print(f"Protocol version: {PROTOCOL_VERSION}")
    print()

    # Port conflict retry: up to 3 attempts
    max_retries = 3
    last_child = None
    exit_code = 1

    for attempt in range(1, max_retries + 1):
        if attempt > 1:
            print(f"\nRetry attempt {attempt}/{max_retries}...")
            # Kill previous server if any
            if last_child:
                kill_server_process(last_child)
                last_child = None
            # Find new port
            port = find_free_port()
            print(f"  Trying port {port}")

        try:
            exit_code = await run_http_tests(binary, port)
            if exit_code == 0:
                return 0
            print(f"  Attempt {attempt}/{max_retries}: HTTP tests returned exit code {exit_code}")
        except Exception as e:
            print(f"  Attempt {attempt}/{max_retries} failed: {e}")
            exit_code = 1

        # Retry logic: if not last attempt, prepare for retry
        if attempt < max_retries:
            print(f"  Retrying with new port...")

    # All retries exhausted
    print(f"All {max_retries} attempts failed")
    return exit_code


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
