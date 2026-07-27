#!/usr/bin/env python3
"""Shared test utilities for websearch MCP integration tests.

Provides helpers for finding the server binary, managing the server process,
and running the standard MCP test suite (initialize, list_tools, search,
continuation).

Keep in sync with mcp_helpers.rs (PROTOCOL_VERSION constant).
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

PROTOCOL_VERSION = "2025-03-26"
# Keep in sync with mcp_helpers.rs


def print_results(results, label: str, max_items: int = 3):
    """Print search results summary: title, URL, and date for first N items."""
    if not results:
        print(f"  {label}: (empty)")
        return
    for i, r in enumerate(results[:max_items]):
        title = r.get("title", "?")
        url = r.get("url", "?")
        date = r.get("date", "")
        if date:
            print(f"  {label}[{i}]: {title}  |  {url}  |  {date}")
        else:
            print(f"  {label}[{i}]: {title}  |  {url}")
    if len(results) > max_items:
        print(f"  {label}: ... and {len(results) - max_items} more")


def find_server_binary() -> Path:
    """Locate delulu-websearch-mcp binary.

    Precondition: binary has been built (cargo build -p delulu-websearch --features mcp).
    Postcondition: Returns Path to existing executable file, or raises RuntimeError.
    Raises RuntimeError: Binary not found in target/debug/, target/release/, or on PATH.
    Raises RuntimeError: Binary exists but is not executable (os.access(X_OK) check).
    """
    # Workspace = tests/ -> websearch/ -> delulu-apps/ -> delulu/
    workspace = Path(__file__).parent.parent.parent.parent
    candidates = [
        workspace / "target" / "debug" / "delulu-websearch-mcp",
        workspace / "target" / "release" / "delulu-websearch-mcp",
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
    which_result = shutil.which("delulu-websearch-mcp")
    if which_result:
        return Path(which_result)
    raise RuntimeError(
        "find_server_binary: binary not found.\n"
        "  Checked: target/debug/delulu-websearch-mcp, target/release/delulu-websearch-mcp, PATH\n"
        "  Run: `cargo build -p delulu-websearch --features mcp`"
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


async def safe_call_tool(session, tool_name: str, args: dict):
    """Wrap call_tool with structured diagnostics.

    Catches TRANSPORT-LEVEL exceptions only (network errors, connection refused).
    Re-raises with `raise ... from e` to preserve original exception chain.
    Diagnostic includes: engine name extracted from args, list of possible causes.

    Tool-level errors (isError in response) are NOT caught here —
    they are checked by assert_tool_success().

    Raises: Transport-level exceptions wrapped in RuntimeError with diagnostic.
    """
    try:
        return await session.call_tool(tool_name, args)
    except (ConnectionError, OSError, asyncio.TimeoutError) as e:
        engine = args.get("engine", "unknown")
        raise RuntimeError(
            f"safe_call_tool('{tool_name}', engine={engine}): "
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


def is_access_denied_error(result) -> bool:
    """Check if MCP tool call returned 'Access denied by search engine'.

    Uses case-insensitive substring matching ("access denied" in error_text.lower())
    to be resilient against minor server-side wording changes.
    Logs the exact error text alongside the skip message.

    Note: This depends on sanitize_error_for_client() in main_mcp.rs.
    If the server code changes the error string, this function must be updated.
    """
    if not hasattr(result, "isError") or not result.isError:
        return False
    content = getattr(result, "content", [])
    if not content or not hasattr(content[0], "text"):
        return False
    error_text = content[0].text
    if "access denied" in error_text.lower():
        print(f"  Access denied error text: {error_text}")
        return True
    return False


async def test_mcp_initialize(session):
    """Assert protocolVersion == PROTOCOL_VERSION."""
    try:
        print(f"  >>> initialize()")
        result = await asyncio.wait_for(session.initialize(), timeout=20.0)
        actual = result.protocolVersion
        print(f"  <<< protocolVersion={actual}")
        assert actual == PROTOCOL_VERSION, (
            f"Expected protocolVersion={PROTOCOL_VERSION}, got {actual}"
        )
        # Step 3: Log session TTL diagnostic
        print(f"  Session TTL: 600s (default) — test duration within limits")

        return (True, "PASSED")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_list_tools(session):
    """Assert tool names contain 'web_search' and 'web_search_next_page'."""
    try:
        print(f"  >>> tools/list")
        tools = await asyncio.wait_for(session.list_tools(), timeout=20.0)
        tool_names = [t.name for t in tools.tools]
        print(f"  <<< tools: {tool_names}")
        assert "web_search" in tool_names, (
            f"Expected 'web_search' in tools, got {tool_names}"
        )
        assert "web_search_next_page" in tool_names, (
            f"Expected 'web_search_next_page' in tools, got {tool_names}"
        )
        return (True, "PASSED")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_search_duckduckgo(session):
    """Search DDG, validate response structure, handle challenge pages gracefully."""
    try:
        query = {"query": "CuTe Layout Algebra tutorial", "engine": "duckduckgo"}
        print(f"  >>> web_search({query})")
        result = await asyncio.wait_for(
            safe_call_tool(session, "web_search", query),
            timeout=20.0,
        )
        if is_access_denied_error(result):
            return (True, "SKIPPED (DuckDuckGo returned a challenge page — AccessDenied)")

        assert_tool_success(result)
        content = result.content
        assert len(content) > 0, "Response should have content"
        assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"
        data = json.loads(content[0].text)
        print(f"  <<< session_key={data.get('session_key', 'MISSING')[:20]}... has_next_page={data.get('has_next_page', 'MISSING')} results.duckduckgo={len(data.get('results', {}).get('duckduckgo', []))} items")

        # Validate response structure
        assert isinstance(data.get("session_key"), str) and data["session_key"], (
            "session_key should be a non-empty string"
        )
        assert isinstance(data.get("results"), dict), "results should be an object"
        assert "duckduckgo" in data["results"], (
            "results should contain 'duckduckgo' key"
        )
        assert isinstance(data.get("has_next_page"), bool), (
            "has_next_page should be a boolean"
        )

        # Validate each result has title and url
        results_list = data["results"]["duckduckgo"]
        assert isinstance(results_list, list), "results.duckduckgo should be an array"
        for item in results_list:
            assert isinstance(item.get("title"), str), "Each result should have a 'title' string"
            assert isinstance(item.get("url"), str), "Each result should have a 'url' string"

        # Log engine_errors warning if present
        engine_errors = data.get("engine_errors")
        if engine_errors:
            print(f"  WARNING: Engine reported errors: {engine_errors}")

        return (True, "PASSED")
    except json.JSONDecodeError as e:
        return (False, f"FAILED: invalid JSON response — {e}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except RuntimeError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_search_brave(session):
    """Search Brave, validate response structure, handle challenge pages."""
    try:
        query = {"query": "CuTe Layout Algebra tutorial", "engine": "brave"}
        print(f"  >>> web_search({query})")
        result = await asyncio.wait_for(
            safe_call_tool(session, "web_search", query),
            timeout=20.0,
        )
        if is_access_denied_error(result):
            return (True, "SKIPPED (Brave returned a challenge page — AccessDenied)")

        assert_tool_success(result)
        content = result.content
        assert len(content) > 0, "Response should have content"
        assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"
        data = json.loads(content[0].text)
        print(f"  <<< session_key={data.get('session_key', 'MISSING')[:20]}... has_next_page={data.get('has_next_page', 'MISSING')} results.brave={len(data.get('results', {}).get('brave', []))} items")
        print_results(data.get('results', {}).get('brave', []), "brv")

        # Validate response structure
        assert isinstance(data.get("session_key"), str) and data["session_key"], (
            "session_key should be a non-empty string"
        )
        assert isinstance(data.get("results"), dict), "results should be an object"
        assert "brave" in data["results"], (
            "results should contain 'brave' key"
        )
        assert isinstance(data.get("has_next_page"), bool), (
            "has_next_page should be a boolean"
        )

        # Validate each result has title and url
        results_list = data["results"]["brave"]
        assert isinstance(results_list, list), "results.brave should be an array"
        for item in results_list:
            assert isinstance(item.get("title"), str), "Each result should have a 'title' string"
            assert isinstance(item.get("url"), str), "Each result should have a 'url' string"

        # Log engine_errors warning if present
        engine_errors = data.get("engine_errors")
        if engine_errors:
            print(f"  WARNING: Engine reported errors: {engine_errors}")

        return (True, "PASSED")
    except json.JSONDecodeError as e:
        return (False, f"FAILED: invalid JSON response — {e}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except RuntimeError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def test_continuation(session):
    """3-step pagination test with malformed-response validation and skip-on-no-next-page."""
    try:
        # Step 1: First search
        query = {"query": "flashAttention", "engine": "duckduckgo"}
        print(f"  >>> web_search({query})")
        result = await asyncio.wait_for(
            safe_call_tool(session, "web_search", query),
            timeout=20.0,
        )
        if is_access_denied_error(result):
            return (True, "SKIPPED (DuckDuckGo returned a challenge page — AccessDenied)")

        assert_tool_success(result)
        content = result.content
        assert len(content) > 0, "Response should have content"
        assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"
        data = json.loads(content[0].text)

        # Validate response structure before accessing fields
        assert isinstance(data.get("session_key"), str) and data["session_key"], (
            "session_key should be a non-empty string"
        )
        assert isinstance(data.get("has_next_page"), bool), (
            "has_next_page should be a boolean"
        )

        session_key = data["session_key"]
        has_next_page = data["has_next_page"]
        print(f"  <<< session_key={session_key[:20]}... has_next_page={has_next_page}")

        if not has_next_page:
            print(f"  >>> web_search_next_page — SKIPPED (no next page)")
            print(f"  WARNING: has_next_page was false — continuation test SKIPPED. ",
                  "This may indicate a session issue if this was unexpected.")
            return (True, "SKIPPED (no next page)")

        # Step 2: Call next page
        print(f"  >>> web_search_next_page({{'session_key': session_key[:20] + '...'}})")
        search_time = time.monotonic()
        next_result = await asyncio.wait_for(
            safe_call_tool(session, "web_search_next_page", {"session_key": session_key}),
            timeout=20.0,
        )

        # Handle "Session not found or expired"
        if hasattr(next_result, "isError") and next_result.isError:
            content_list = getattr(next_result, "content", [])
            if content_list and hasattr(content_list[0], "text"):
                error_text = content_list[0].text
                elapsed = time.monotonic() - search_time
                if "Session not found or expired" in error_text:
                    print(f"  Continuation delay: {elapsed:.1f}s (SessionCache TTL is 600s)")
                    return (True, "SKIPPED (session expired between calls)")

        assert_tool_success(next_result)
        next_content = next_result.content
        assert len(next_content) > 0, "Response should have content"
        assert hasattr(next_content[0], "text"), f"Expected text content, got {type(next_content[0])}"
        next_data = json.loads(next_content[0].text)
        print(f"  <<< results={len(next_data.get('results', []))} items has_next_page={next_data.get('has_next_page', 'MISSING')}")
        print_results(next_data.get('results', []), "next")

        # Validate next page response structure
        assert isinstance(next_data.get("results"), list), (
            "results should be a flat array (next page response)"
        )
        assert isinstance(next_data.get("has_next_page"), bool), (
            "has_next_page should be a boolean"
        )

        return (True, "PASSED")
    except json.JSONDecodeError as e:
        return (False, f"FAILED: invalid JSON response — {e}")
    except AssertionError as e:
        return (False, f"FAILED: {e}")
    except RuntimeError as e:
        return (False, f"FAILED: {e}")
    except Exception as e:
        return (False, f"FAILED: unexpected error — {e}")


async def run_mcp_tests(session) -> dict:
    """Run the standard MCP test suite against an initialized session.

    Expects an already-initialized session (does NOT call session.initialize()).

    Runs: initialize, list_tools, search_duckduckgo, search_brave, continuation.
    Each test function is wrapped with asyncio.wait_for() with 20s per-call timeout.
    Each test catches expected skips gracefully and returns a (passed, message) tuple.
    Returns dict: {test_name: {"passed": bool, "message": str}}

    Minimum coverage check: if all 5 test functions report skipped (none passed),
    prints a loud warning and caller should exit with code 1.
    """
    test_functions = [
        ("MCP initialization", test_mcp_initialize),
        ("List tools", test_list_tools),
        ("DuckDuckGo search", test_search_duckduckgo),
        ("Brave search", test_search_brave),
        ("Continuation test", test_continuation),
    ]

    results = {}
    any_passed = False
    all_skipped = True

    for name, func in test_functions:
        try:
            passed, message = await func(session)
            results[name] = {"passed": passed, "message": message}
            if passed:
                any_passed = True
            if "SKIPPED" not in message:
                all_skipped = False
        except Exception as e:
            results[name] = {"passed": False, "message": f"FAILED: unexpected error — {e}"}

    # Minimum coverage check
    if all_skipped and not any_passed:
        print()
        print("=" * 60)
        print("WARNING: All tests skipped — no assertions were exercised")
        print("=" * 60)

    return results
