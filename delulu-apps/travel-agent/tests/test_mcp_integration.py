#!/usr/bin/env python3
"""MCP server integration tests using Python MCP SDK."""

import asyncio
import json
import os
import signal
import sys
from datetime import date, timedelta
from pathlib import Path

from mcp.client.stdio import stdio_client, StdioServerParameters
from mcp import ClientSession


def kill_server_processes() -> None:
    """Kill any leftover server processes using process group."""
    if sys.platform != "win32":
        try:
            signal.pthread_sigmask(signal.SIG_BLOCK, (signal.SIGTERM,))
            os.killpg(0, signal.SIGTERM)
        except (ProcessLookupError, OSError):
            pass


def find_server_binary() -> Path:
    """Find the delulu-travel-mcp binary."""
    workspace = Path(__file__).parent.parent.parent.parent
    for debug in [
        workspace / "target" / "debug" / "delulu-travel-mcp",
        workspace / "target" / "release" / "delulu-travel-mcp",
    ]:
        if debug.exists():
            return debug
    raise RuntimeError(
        "Could not find delulu-travel-mcp binary. Run `cargo build -p delulu-travel-agent --features mcp` first."
    )


def future_date(months_ahead: int) -> str:
    """Get a future date in YYYY-MM-DD format."""
    d = date.today() + timedelta(days=30 * months_ahead)
    return d.isoformat()


async def test_mcp_initialize(session) -> bool:
    """Test MCP initialization."""
    print("Testing MCP initialization...")
    result = await session.initialize()
    print(f"✓ Initialized with protocol version: {result.protocolVersion}")
    return True


async def test_list_tools(session) -> bool:
    """Test listing available tools."""
    print("Testing list_tools...")
    tools = await session.list_tools()
    tool_names = [t.name for t in tools.tools]
    print(f"✓ Found {len(tools.tools)} tools: {tool_names}")
    assert "search_flights" in tool_names, "search_flights tool should be available"
    assert "search_hotels" in tool_names, "search_hotels tool should be available"
    return True


async def test_search_flights(session) -> bool:
    """Test searching flights via MCP."""
    print("\nTesting search_flights...")

    depart_date = future_date(2)
    return_date = (date.fromisoformat(depart_date) + timedelta(days=7)).isoformat()

    print(f"Query: YYZ → CDG on {depart_date} (return {return_date})")

    result = await session.call_tool(
        "search_flights",
        {
            "from": "YYZ",
            "to": "CDG",
            "date": depart_date,
            "return_date": return_date,
            "seat": "Economy",
            "adults": 1,
            "children_ages": [5, 8],
            "trip_type": "round-trip",
            "max_stops": 1,
        },
    )

    content = result.content
    if hasattr(content[0], "text"):
        text = content[0].text
        print(f"Response length: {len(text)} chars")

        data = json.loads(text)
        print(f"✓ Got response")

        assert "search_flights" in data, "Response should contain search_flights"

        sf = data["search_flights"]
        assert "total" in sf, "search_flights should contain 'total'"
        assert "query" in sf, "search_flights should contain 'query'"
        assert "results" in sf, "search_flights should contain 'results'"

        query = sf["query"]
        assert query["from"] == "YYZ", "from should be YYZ"
        assert query["to"] == "CDG", "to should be CDG"

        results = sf["results"]
        assert isinstance(results, list), "results should be a list"

        if results:
            first = results[0]
            assert "price" in first, "result should have price"
            assert "currency" in first, "result should have currency"
            assert "airlines" in first, "result should have airlines"
            assert "route" in first, "result should have route"

        print(
            f"✓ Response schema validated: total={sf['total']}, results={len(results)}"
        )
        return True
    else:
        print(f"✗ Unexpected response type: {type(content[0])}")
        return False


async def test_search_hotels(session) -> bool:
    """Test searching hotels via MCP."""
    print("\nTesting search_hotels...")

    checkin = future_date(1)
    checkout = (date.fromisoformat(checkin) + timedelta(days=3)).isoformat()

    print(f"Query: Paris, {checkin} to {checkout}")

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
    if hasattr(content[0], "text"):
        text = content[0].text
        print(f"Response length: {len(text)} chars")

        data = json.loads(text)
        print(f"✓ Got response")

        assert "search_hotels" in data, "Response should contain search_hotels"

        sh = data["search_hotels"]
        assert "total" in sh, "search_hotels should contain 'total'"
        assert "query" in sh, "search_hotels should contain 'query'"
        assert "results" in sh, "search_hotels should contain 'results'"

        query = sh["query"]
        assert query["location"] == "Paris", "location should be Paris"

        results = sh["results"]
        assert isinstance(results, list), "results should be a list"

        if results:
            first = results[0]
            assert "name" in first, "result should have name"
            assert "address" in first, "result should have address"
            assert "price" in first, "result should have price"
            assert "currency" in first, "result should have currency"
            assert "rating" in first, "result should have rating"
            assert "stars" in first, "result should have stars"
            assert "amenities" in first, "result should have amenities"

        print(
            f"✓ Response schema validated: total={sh['total']}, results={len(results)}"
        )
        return True
    else:
        print(f"✗ Unexpected response type: {type(content[0])}")
        return False


async def run_tests() -> int:
    """Run all MCP integration tests."""
    print("=" * 60)
    print("MCP Server Integration Tests")
    print("=" * 60)

    server_binary = find_server_binary()
    print(f"Using server binary: {server_binary}")
    print()

    server_params = StdioServerParameters(
        command=str(server_binary),
        args=["stdio"],
        env=None,
    )

    try:
        async with stdio_client(server_params) as (read, write):
            async with ClientSession(read, write) as session:
                await asyncio.sleep(0.5)

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
        kill_server_processes()


if __name__ == "__main__":
    sys.exit(asyncio.run(run_tests()))
