#!/usr/bin/env python3
"""MCP stdio transport integration tests for delulu-websearch-mcp.

Connects to the MCP server via stdio transport and tests the full MCP lifecycle:
initialize → list_tools → web_search (DuckDuckGo) → web_search (Brave) →
continuation (web_search_next_page).

All tests are live (hit real search engines).

Usage:
    python3 tests/test_websearch_stdio.py [binary_path]

Exit codes:
    0 = all passed (or gracefully skipped with at least one test passing)
    1 = test failure or all tests skipped
    2 = environment error (missing SDK)
"""

import asyncio
import sys
from pathlib import Path

# MCP Python SDK imports (explicit paths)
try:
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client
except ImportError:
    print("FAIL: mcp Python SDK not installed. Run: pip install 'mcp>=1.0.0'")
    sys.exit(2)

sys.path.insert(0, str(Path(__file__).parent))
from websearch_test_utils import (
    find_server_binary,
    run_mcp_tests,
    PROTOCOL_VERSION,
)


async def main():
    """Run MCP stdio tests."""
    # Determine binary path
    if len(sys.argv) >= 2:
        binary = Path(sys.argv[1])
    else:
        binary = find_server_binary()

    print("=" * 60)
    print("MCP Server Integration Tests (stdio)")
    print("=" * 60)
    print(f"Using server binary: {binary}")
    print(f"Protocol version: {PROTOCOL_VERSION}")
    print()

    params = StdioServerParameters(command=str(binary), args=["stdio"], env=None)

    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            results = await run_mcp_tests(session)

    # Print results
    print()
    print("=" * 60)
    print("Results:")
    print("=" * 60)
    tests_passed = 0
    tests_skipped = 0
    tests_failed = 0
    tests_total = len(results)
    for name, result in results.items():
        status = result["message"]
        print(f"  {name}: {status}")
        if result.get("skipped", "SKIPPED" in status):
            tests_skipped += 1
        elif result["passed"]:
            tests_passed += 1
        else:
            tests_failed += 1

    print()
    print(f"  Passed: {tests_passed}/{tests_total}, Skipped: {tests_skipped}, Failed: {tests_failed}")
    print("=" * 60)

    # Compute exit code
    if tests_failed > 0:
        return 1
    elif tests_passed > 0:
        return 0
    else:
        # All tests skipped — eligible for retry
        print("ERROR: All tests failed or were skipped — no assertions exercised")
        return 1


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
