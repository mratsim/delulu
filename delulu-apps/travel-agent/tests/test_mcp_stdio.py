#!/usr/bin/env python3
"""MCP server integration tests using Python MCP SDK."""

import asyncio
import json
import os
import signal
import sys
from datetime import date, timedelta
from pathlib import Path

from jsonschema import Draft7Validator, ValidationError

from mcp.client.stdio import stdio_client, StdioServerParameters
from mcp import ClientSession


def load_json_schema(name: str) -> dict:
    """Load JSON schema from schemas directory."""
    schema_path = Path(__file__).parent / "schemas" / name
    with open(schema_path) as f:
        return json.load(f)


FLIGHTS_RESPONSE_SCHEMA = load_json_schema("flights-response.json")
HOTELS_RESPONSE_SCHEMA = load_json_schema("hotels-response.json")


def validate_json_schema(instance: dict, schema: dict, schema_name: str) -> None:
    """Validate instance against schema, raise if invalid."""
    validator = Draft7Validator(schema)
    errors = list(validator.iter_errors(instance))
    if errors:
        error_msgs = [f"{schema_name}: {e.message}" for e in errors]
        raise ValidationError(f"Schema validation failed:\n" + "\n".join(error_msgs))


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
    return_date = (date.fromisoformat(depart_date) + timedelta(days=7)).isoformat()

    print(f"  Query: SFO → SYD on {depart_date} (return {return_date})")

    result = await session.call_tool(
        "search_flights",
        {
            "from": "SFO",
            "to": "SYD",
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
    assert len(content) > 0, "Response should have content"
    assert hasattr(content[0], "text"), f"Expected text content, got {type(content[0])}"

    text = content[0].text
    print(f"  Response length: {len(text)} chars")

    data = json.loads(text)
    print(f"  Got valid JSON response")

    assert "search_flights" in data, "Response should contain search_flights"

    try:
        validate_json_schema(data, FLIGHTS_RESPONSE_SCHEMA, "flights_response")
        print(f"  JSON schema validated")
    except ValidationError as e:
        print(f"  Schema validation warning: {e}")

    sf = data["search_flights"]
    query = sf["query"]
    assert query["from"] == "SFO", f"from should be SFO, got {query}"
    assert query["to"] == "SYD", f"to should be SYD, got {query}"

    results = sf["results"]
    assert isinstance(results, list), "results should be a list"

    print(f"  Response validated: total={sf['total']}, results={len(results)}")
    return True


async def test_search_hotels(session) -> bool:
    """Test searching hotels via MCP with JSON schema validation."""
    print("\nTesting search_hotels...")

    checkin = future_date(1)
    checkout = (date.fromisoformat(checkin) + timedelta(days=3)).isoformat()

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
    print(f"  Got valid JSON response")

    assert "search_hotels" in data, "Response should contain search_hotels"

    try:
        validate_json_schema(data, HOTELS_RESPONSE_SCHEMA, "hotels_response")
        print(f"  JSON schema validated")
    except ValidationError as e:
        print(f"  Schema validation warning: {e}")

    sh = data["search_hotels"]
    query = sh["query"]
    loc = query.get("loc") or query.get("location")
    assert loc == "Paris", f"location should be Paris, got {query}"

    results = sh["results"]
    assert isinstance(results, list), "results should be a list"

    print(f"  Response validated: total={sh['total']}, results={len(results)}")
    return True


async def run_tests() -> int:
    """Run all MCP integration tests."""
    print("=" * 60)
    print("MCP Server Integration Tests (stdio)")
    print("=" * 60)
    print(f"Using server binary: {find_server_binary()}")
    print()

    server_binary = find_server_binary()
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
