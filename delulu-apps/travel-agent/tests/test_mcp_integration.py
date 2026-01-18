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

    print(f"Query: SFO → JFK on {depart_date} (return {return_date})")

    result = await session.call_tool(
        "search_flights",
        {
            "from_airport": "SFO",
            "to_airport": "JFK",
            "depart_date": depart_date,
            "return_date": return_date,
            "cabin_class": "economy",
            "adults": 1,
            "children_ages": [],
            "trip_type": "round_trip",
        },
    )

    content = result.content
    if hasattr(content[0], "text"):
        text = content[0].text
        print(f"Response length: {len(text)} chars")

        data = json.loads(text)
        print(f"✓ Got response with {len(data.get('itineraries', []))} itineraries")

        assert "itineraries" in data, "Response should contain itineraries"
        assert "raw_response" in data, "Response should contain raw_response"
        assert "search_params" in data, "Response should contain search_params"

        params = data["search_params"]
        assert params["from_airport"] == "SFO", "from_airport should match"
        assert params["to_airport"] == "JFK", "to_airport should match"

        return True
    else:
        print(f"✗ Unexpected response type: {type(content[0])}")
        return False


async def test_search_hotels(session) -> bool:
    """Test searching hotels via MCP."""
    print("\nTesting search_hotels...")

    checkin = future_date(1)
    checkout = (date.fromisoformat(checkin) + timedelta(days=3)).isoformat()

    print(f"Query: New York, {checkin} to {checkout}")

    result = await session.call_tool(
        "search_hotels",
        {
            "location": "New York",
            "checkin_date": checkin,
            "checkout_date": checkout,
            "adults": 2,
            "children_ages": [],
            "currency": "USD",
        },
    )

    content = result.content
    if hasattr(content[0], "text"):
        text = content[0].text
        print(f"Response length: {len(text)} chars")

        data = json.loads(text)
        print(f"✓ Got response with {len(data.get('hotels', []))} hotels")

        assert "hotels" in data, "Response should contain hotels"
        assert "lowest_price" in data, "Response should contain lowest_price"

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
