#!/usr/bin/env python3
"""Shared test utilities for MCP integration tests."""

import json
from datetime import date, timedelta
from pathlib import Path

from jsonschema import Draft7Validator, ValidationError


def load_json_schema(name: str) -> dict:
    """Load JSON schema from schemas directory."""
    schema_path = Path(__file__).parent.parent / "src" / "schemas" / name
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
        raise ValidationError("Schema validation failed:\n" + "\n".join(error_msgs))


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
