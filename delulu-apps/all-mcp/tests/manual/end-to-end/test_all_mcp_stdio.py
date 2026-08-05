#!/usr/bin/env python3
"""MCP stdio transport integration tests for delulu-all-mcp.

Spawns `delulu-all-mcp stdio` and runs the shared suite (initialize,
list_tools, and the error paths) through the official MCP Python SDK.

Usage:
    python3 test_all_mcp_stdio.py <binary_path>

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
from all_mcp_test_utils import (
    find_server_binary,
    print_results,
    run_all_mcp_tests,
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
    print("delulu-all-mcp Integration Tests (stdio)")
    print("=" * 60)
    print(f"Using server binary: {binary}")
    print(f"Protocol version: {PROTOCOL_VERSION}")
    print()

    server_args = ["stdio"]
    params = StdioServerParameters(command=str(binary), args=server_args, env=None)

    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            results = await run_all_mcp_tests(session)

    return print_results(results)


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
