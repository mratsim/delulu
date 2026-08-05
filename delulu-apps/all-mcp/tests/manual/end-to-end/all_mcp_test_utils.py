#!/usr/bin/env python3
"""Shared test utilities for delulu-all-mcp MCP integration tests.

Provides helpers for finding the server binary, managing the server process,
and running the shared MCP test suite (initialize, list_tools, the three
offline fixture-backed paper tools, and the error paths).

Keep in sync with t_all_mcp_python_e2e.rs (PROTOCOL_VERSION constant and the
fixture base-url flag contract).
"""

import asyncio
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

from mcp.shared.exceptions import McpError  # mcp 1.x (pinned <2)

PROTOCOL_VERSION = "2025-03-26"
# Keep in sync with t_all_mcp_python_e2e.rs

EXPECTED_TOOL_COUNT = 21
PREFIXED_GET_PAPER_TOOLS = ("arxiv_get_paper", "iacr_get_paper", "pubmed_get_paper")


def find_server_binary() -> Path:
    """Locate delulu-all-mcp binary (release first, then debug).

    Precondition: binary has been built (cargo build -p delulu-all-mcp --features mcp).
    Postcondition: Returns Path to existing executable file, or raises RuntimeError.
    Raises RuntimeError: Binary not found in target/release/, target/debug/, or on PATH.
    Raises RuntimeError: Binary exists but is not executable (os.access(X_OK) check).
    """
    # Workspace = end-to-end/ -> manual/ -> tests/ -> all-mcp/ -> delulu-apps/ -> delulu/
    workspace = Path(__file__).parent.parent.parent.parent.parent
    candidates = [
        workspace / "target" / "release" / "delulu-all-mcp",
        workspace / "target" / "debug" / "delulu-all-mcp",
    ]
    for c in candidates:
        if c.exists():
            if os.access(c, os.X_OK):
                return c
            raise RuntimeError(
                f"find_server_binary: {c} exists but is not executable.\n"
                f"  Run: chmod +x {c}"
            )
    # Fallback to PATH
    which_result = shutil.which("delulu-all-mcp")
    if which_result:
        return Path(which_result)
    raise RuntimeError(
        "find_server_binary: binary not found.\n"
        "  Checked: target/release/delulu-all-mcp, target/debug/delulu-all-mcp, PATH\n"
        "  Run: `cargo build -p delulu-all-mcp --features mcp`"
    )


def wait_for_server(port: int, timeout: float = 15.0) -> None:
    """Poll TCP port until server accepts connections.

    Precondition: Server process has been spawned and will bind to `port`.
    Postcondition: Server is accepting TCP connections on `port`, or RuntimeError is raised.
    Raises RuntimeError: Timeout expires (with diagnostic listing possible causes).
    Raises RuntimeError: EADDRINUSE detected (port conflict — will never succeed).
    Note: Uses time.monotonic() for clock-skew resistance.
    Note: Distinguishes ECONNREFUSED (transient, retry) from EADDRINUSE (permanent, fail).
    """
    start = time.monotonic()
    last_error = None
    while time.monotonic() - start < timeout:
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(0.5)
            s.connect(("127.0.0.1", port))
            s.close()
            return
        except OSError as e:
            last_error = e
            if e.errno == socket.errno.EADDRINUSE:
                raise RuntimeError(
                    f"wait_for_server(port={port}): EADDRINUSE — "
                    f"port is already bound by another process and will never become available. "
                    f"Check for orphaned server processes."
                ) from e
            # ECONNREFUSED (errno 111) is transient — retry
            time.sleep(0.1)
    raise RuntimeError(
        f"wait_for_server(port={port}): timeout after {timeout}s. "
        f"Server did not start listening. Possible causes:\n"
        f"  - Server process crashed immediately\n"
        f"  - Wrong port (check --port argument)\n"
        f"  - Firewall blocking localhost connections\n"
        f"  - Binary not found or not executable\n"
        f"  Last error: {last_error}"
    )


def kill_server_process(child: subprocess.Popen) -> None:
    """Kill server process and all children via process group.

    Strategy (in order):
    1. SIGTERM to process group — allows graceful shutdown
    2. wait(timeout=2) — gives server time to release port
    3. SIGKILL — forcible cleanup if server is stuck

    Without step 3, a stuck server holds the port for up to 60s (TCP TIME_WAIT).

    Precondition: `child` is a Popen object representing a live or recently exited process.
    Postcondition: Process is terminated. No orphan processes remain.
    Raises: Process does not exist (handled gracefully).
    """
    if child.poll() is not None:
        return  # already exited

    if sys.platform != "win32":
        try:
            # Step 1: SIGTERM to process group
            os.killpg(os.getpgid(child.pid), signal.SIGTERM)
        except (ProcessLookupError, OSError):
            pass

    try:
        # Step 2: Wait for graceful shutdown
        child.wait(timeout=2)
    except subprocess.TimeoutExpired:
        try:
            # Step 3: SIGKILL — forcible cleanup
            if sys.platform != "win32":
                os.killpg(os.getpgid(child.pid), signal.SIGKILL)
            else:
                child.kill()
        except (ProcessLookupError, OSError):
            pass
        child.wait()


def print_results(results: dict) -> int:
    """Print per-test results and compute the exit code.

    Exit code convention:
        0 = at least one test passed and none failed
        1 = a test failed, or all tests were skipped (no assertions exercised)
    """
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

    if tests_failed > 0:
        return 1
    elif tests_passed > 0:
        return 0
    else:
        # All tests skipped — eligible for retry
        print("ERROR: All tests failed or were skipped — no assertions exercised")
        return 1


async def safe_call_tool(session, tool_name: str, args: dict):
    """Wrap call_tool with structured diagnostics.

    Catches TRANSPORT-LEVEL exceptions only (network errors, connection refused).
    Re-raises with `raise ... from e` to preserve original exception chain.

    Tool-level errors (isError in response) are NOT caught here —
    they are checked by assert_tool_success().

    Raises: Transport-level exceptions wrapped in RuntimeError with diagnostic.
    """
    try:
        return await session.call_tool(tool_name, args)
    except (ConnectionError, OSError, asyncio.TimeoutError) as e:
        raise RuntimeError(
            f"safe_call_tool('{tool_name}'): "
            f"transport error — {e}. Possible causes: "
            f"server crashed, network unavailable, port conflict."
        ) from e


def assert_tool_success(result) -> None:
    """Assert that a tool call result does not indicate an error.

    Checks result.isError and prints diagnostic if set.
    """
    if hasattr(result, "isError") and result.isError:
        content = getattr(result, "content", [])
        error_text = ""
        if content and hasattr(content[0], "text"):
            error_text = content[0].text
        raise AssertionError(
            f"Tool call returned isError=true.\n"
            f"  Error text: {error_text}"
        )


async def test_mcp_initialize(session):
    """Assert protocolVersion == PROTOCOL_VERSION."""
    try:
        print("  >>> initialize()")
        result = await asyncio.wait_for(session.initialize(), timeout=20.0)
        actual = result.protocolVersion
        print(f"  <<< protocolVersion={actual}")
        assert actual == PROTOCOL_VERSION, (
            f"Expected protocolVersion={PROTOCOL_VERSION}, got {actual}"
        )
        return (True, "PASSED")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_list_tools(session):
    """Assert the 21-tool union (3 prefixed get_paper, no bare get_paper) and
    that every tool carries a well-formed inputSchema (schemars output)."""
    try:
        print("  >>> tools/list")
        tools = await asyncio.wait_for(session.list_tools(), timeout=20.0)
        tool_names = [t.name for t in tools.tools]
        print(f"  <<< {len(tool_names)} tools")
        assert len(tool_names) == EXPECTED_TOOL_COUNT, (
            f"Expected {EXPECTED_TOOL_COUNT} tools, got {len(tool_names)}: {tool_names}"
        )
        for name in PREFIXED_GET_PAPER_TOOLS:
            assert name in tool_names, f"Expected '{name}' in tools, got {tool_names}"
        assert "get_paper" not in tool_names, (
            f"Bare 'get_paper' must be renamed to the 3 prefixed tools, got {tool_names}"
        )
        # Each tool's inputSchema must be a JSON object (schemars-derived); the
        # SDK surfaces it through list_tools, so a missing/invalid schema here
        # means the serialization boundary broke. (Some schemas are anyOf-style
        # without a top-level `type` — the object check is the contract.)
        for tool in tools.tools:
            schema = tool.inputSchema
            assert schema is not None and isinstance(schema, dict), (
                f"Tool '{tool.name}' inputSchema missing or not an object: {schema}"
            )
        return (True, "PASSED")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_search_papers(session):
    """search_papers against the fixture-served arXiv API (offline)."""
    try:
        args = {"query": "all:electron"}
        print(f"  >>> search_papers({args})")
        result = await asyncio.wait_for(
            safe_call_tool(session, "search_papers", args),
            timeout=30.0,
        )
        assert_tool_success(result)
        content = result.content
        assert len(content) > 0, "Response should have content"
        assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"
        data = json.loads(content[0].text)
        assert isinstance(data, list) and len(data) > 0, (
            f"Expected a non-empty paper list, got {data}"
        )
        assert "title" in data[0], f"Paper missing 'title' field: {data[0]}"
        print(f"  <<< search_papers returned {len(data)} papers")
        return (True, "PASSED")
    except json.JSONDecodeError as e:
        return (False, f"FAILED: invalid JSON response — {e}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except RuntimeError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_list_recent_papers(session):
    """list_recent_papers against the fixture-served IACR RSS feed (offline)."""
    try:
        args = {}
        print(f"  >>> list_recent_papers({args})")
        result = await asyncio.wait_for(
            safe_call_tool(session, "list_recent_papers", args),
            timeout=30.0,
        )
        assert_tool_success(result)
        content = result.content
        assert len(content) > 0, "Response should have content"
        assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"
        data = json.loads(content[0].text)
        assert isinstance(data, list) and len(data) > 0, (
            f"Expected a non-empty paper list, got {data}"
        )
        assert "title" in data[0], f"Paper missing 'title' field: {data[0]}"
        print(f"  <<< list_recent_papers returned {len(data)} papers")
        return (True, "PASSED")
    except json.JSONDecodeError as e:
        return (False, f"FAILED: invalid JSON response — {e}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except RuntimeError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_search_pubmed(session):
    """search_pubmed against the fixture-served PubMed esearch endpoint (offline)."""
    try:
        args = {"query": "test"}
        print(f"  >>> search_pubmed({args})")
        result = await asyncio.wait_for(
            safe_call_tool(session, "search_pubmed", args),
            timeout=30.0,
        )
        assert_tool_success(result)
        content = result.content
        assert len(content) > 0, "Response should have content"
        assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"
        data = json.loads(content[0].text)
        assert isinstance(data.get("total_count"), int) and data["total_count"] > 0, (
            f"Expected total_count > 0, got {data}"
        )
        assert isinstance(data.get("pmids"), list) and len(data["pmids"]) > 0, (
            f"Expected a non-empty pmids list, got {data}"
        )
        print(f"  <<< search_pubmed total_count={data['total_count']} pmids={len(data['pmids'])}")
        return (True, "PASSED")
    except json.JSONDecodeError as e:
        return (False, f"FAILED: invalid JSON response — {e}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except RuntimeError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_no_such_tool(session):
    """Calling an unknown tool must produce an error, not a success result."""
    try:
        print("  >>> call_tool(no_such_tool, {})")
        try:
            result = await asyncio.wait_for(
                session.call_tool("no_such_tool", {}),
                timeout=20.0,
            )
        except McpError as e:
            print(f"  <<< no_such_tool raised McpError: {e}")
            assert "tool not found" in str(e).lower(), (
                f"expected 'tool not found' in error message, got: {e}"
            )
            return (True, "PASSED")
        # Some transports surface tool errors as isError results instead.
        if hasattr(result, "isError") and result.isError:
            text = result.content[0].text if result.content else ""
            print(f"  <<< no_such_tool returned isError: {text}")
            assert "tool not found" in text.lower(), f"unexpected error text: {text}"
            return (True, "PASSED")
        raise AssertionError(f"expected error for unknown tool, got success result: {result}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_get_paper_hint(session):
    """Bare get_paper must be rejected with a did-you-mean hint."""
    try:
        print("  >>> call_tool(get_paper, {})")
        try:
            result = await asyncio.wait_for(
                session.call_tool("get_paper", {}),
                timeout=20.0,
            )
        except McpError as e:
            print(f"  <<< get_paper raised McpError: {e}")
            assert "did you mean" in str(e).lower(), (
                f"expected did-you-mean hint in error message, got: {e}"
            )
            assert "arxiv_get_paper" in str(e), (
                f"hint must list arxiv_get_paper, got: {e}"
            )
            return (True, "PASSED")
        # Some transports surface tool errors as isError results instead.
        if hasattr(result, "isError") and result.isError:
            text = result.content[0].text if result.content else ""
            print(f"  <<< get_paper returned isError: {text}")
            assert "did you mean" in text.lower(), (
                f"expected did-you-mean hint, got: {text}"
            )
            return (True, "PASSED")
        raise AssertionError(f"expected error for bare get_paper, got success result: {result}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def run_all_mcp_tests(session) -> dict:
    """Run the shared all-mcp test suite against an initialized session.

    The MCP ClientSession must already be connected before calling this function.
    The suite includes test_mcp_initialize which calls session.initialize().

    Runs: initialize, list_tools (21-tool union), the three offline
    fixture-backed paper tools (search_papers, list_recent_papers,
    search_pubmed), and the two error paths (unknown tool, get_paper hint).
    Each test function is wrapped with asyncio.wait_for() with a per-call timeout.
    Each test returns a (passed, message) tuple.

    Returns dict: {test_name: {"passed": bool, "skipped": bool, "message": str}}
    """
    test_functions = [
        ("MCP initialization", test_mcp_initialize),
        ("List tools (21-tool union)", test_list_tools),
        ("search_papers (arxiv fixture)", test_search_papers),
        ("list_recent_papers (iacr fixture)", test_list_recent_papers),
        ("search_pubmed (pubmed fixture)", test_search_pubmed),
        ("Unknown tool error", test_no_such_tool),
        ("get_paper did-you-mean hint", test_get_paper_hint),
    ]

    results = {}
    any_passed = False
    all_skipped = True

    for name, func in test_functions:
        try:
            passed, message = await func(session)
            is_skipped = "SKIPPED" in message
            results[name] = {"passed": passed, "skipped": is_skipped, "message": message}
            if passed and not is_skipped:
                any_passed = True
            if not is_skipped:
                all_skipped = False
        except Exception as e:
            results[name] = {"passed": False, "skipped": False, "message": f"FAILED: unexpected error — {e}"}

    # Minimum coverage check
    if all_skipped and not any_passed:
        print()
        print("=" * 60)
        print("WARNING: All tests skipped — no assertions were exercised")
        print("=" * 60)

    return results
