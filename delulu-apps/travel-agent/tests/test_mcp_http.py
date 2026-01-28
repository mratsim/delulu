#!/usr/bin/env python3
"""MCP HTTP transport integration tests using Python MCP SDK."""

import asyncio
import datetime
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

from mcp_test_utils import (
    FLIGHTS_RESPONSE_SCHEMA,
    HOTELS_RESPONSE_SCHEMA,
    find_server_binary,
    future_date,
    validate_json_schema,
)


def wait_for_server(port: int, timeout: float = 5.0) -> None:
    """Poll until server is ready to accept connections."""
    start = time.time()
    while time.time() - start < timeout:
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(0.1)
            s.connect(("127.0.0.1", port))
            s.close()
            return
        except Exception:
            time.sleep(0.05)
    raise RuntimeError(f"Server not ready on port {port}")


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
    result = await session.initialize()
    print(f"  Initialized with protocol version: {result.protocolVersion}")
    assert result.protocolVersion == "2025-03-26", (
        f"Expected 2025-03-26, got {result.protocolVersion}"
    )
    return True


async def test_list_tools(session) -> bool:
    """Test listing available tools."""
    print("Testing list_tools...")
    tools = await session.list_tools()
    tool_names = [t.name for t in tools.tools]
    print(f"  Found {len(tools.tools)} tools: {tool_names}")
    assert "search_flights" in tool_names, "search_flights tool should be available"
    assert "search_hotels" in tool_names, "search_hotels tool should be available"
    return True


async def test_search_flights(session) -> bool:
    """Test searching flights via MCP with JSON schema validation."""
    print("\nTesting search_flights...")

    depart_date = future_date(2)

    print(f"  Query: SFO → JFK on {depart_date}")

    result = await session.call_tool(
        "search_flights",
        {
            "from": "SFO",
            "to": "JFK",
            "date": depart_date,
            "seat": "Economy",
            "adults": 1,
        },
    )

    content = result.content
    assert len(content) > 0, "Response should have content"
    assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"

    text = content[0].text
    print(f"  Response length: {len(text)} chars")

    try:
        data = json.loads(text)
        print("  Got valid JSON response")

        assert "search_flights" in data, "Response should contain search_flights"

        validate_json_schema(data, FLIGHTS_RESPONSE_SCHEMA, "flights_response")
        print("  JSON schema validated")

        sf = data["search_flights"]
        query = sf["query"]
        assert query["from"] == "SFO", f"from should be SFO, got {query}"
        assert query["to"] == "JFK", f"to should be JFK, got {query}"

        results = sf["results"]
        assert isinstance(results, list), "results should be a list"

        print(f"  Response validated: total={sf['total']}, results={len(results)}")
        return True
    except json.JSONDecodeError:
        print("  Response is not JSON (transport works, parser may have failed):")
        print(f"     ====\n    {text}\n====\n")
        raise AssertionError(f"Response is not valid JSON: {text}")


async def test_search_hotels(session) -> bool:
    """Test searching hotels via MCP with JSON schema validation."""
    print("\nTesting search_hotels...")

    checkin = future_date(1)
    checkout = (
        datetime.date.fromisoformat(checkin) + datetime.timedelta(days=3)
    ).isoformat()

    print(f"  Query: Paris, {checkin} to {checkout}")

    result = await session.call_tool(
        "search_hotels",
        {
            "location": "Paris",
            "checkin_date": checkin,
            "checkout_date": checkout,
            "adults": 2,
            "children_ages": [10],
            "min_guest_rating": 4.5,
            "stars": [4, 5],
            "amenities": ["pool", "spa", "gym"],
            "min_price": 100,
            "max_price": 500,
        },
    )

    content = result.content
    assert len(content) > 0, "Response should have content"
    assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"

    text = content[0].text
    print(f"  Response length: {len(text)} chars")

    data = json.loads(text)
    print("  Got valid JSON response")

    assert "search_hotels" in data, "Response should contain search_hotels"

    validate_json_schema(data, HOTELS_RESPONSE_SCHEMA, "hotels_response")
    print("  JSON schema validated")

    sh = data["search_hotels"]
    query = sh["query"]
    loc = query.get("loc") or query.get("location")
    assert loc == "Paris", f"location should be Paris, got {query}"

    results = sh["results"]
    assert isinstance(results, list), "results should be a list"

    print(f"  Response validated: total={sh['total']}, results={len(results)}")
    return True


async def run_http_tests(port: int) -> int:
    """Run all MCP HTTP transport integration tests."""
    print("=" * 60)
    print("MCP HTTP Transport Integration Tests")
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
                tests_total = 4

                if await test_mcp_initialize(session):
                    tests_passed += 1

                if await test_list_tools(session):
                    tests_passed += 1

                if await test_search_flights(session):
                    tests_passed += 1

                if await test_search_hotels(session):
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
            "test_mcp_http.py:244: Server killed before client cleanup - 'Session termination failed' is expected"
        )


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18080
    sys.exit(asyncio.run(run_http_tests(port)))
