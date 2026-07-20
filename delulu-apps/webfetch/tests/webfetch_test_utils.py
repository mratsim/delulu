#!/usr/bin/env python3
"""Shared test utilities for webfetch MCP integration tests."""

import socket
import time
from pathlib import Path


def find_server_binary() -> Path:
    """Find the delulu-webfetch-mcp binary."""
    workspace = Path(__file__).parent.parent.parent.parent  # tests -> webfetch -> delulu-apps -> delulu/
    for candidate in [
        workspace / "target" / "debug" / "delulu-webfetch-mcp",
        workspace / "target" / "release" / "delulu-webfetch-mcp",
    ]:
        if candidate.exists():
            return candidate
    raise RuntimeError(
        "Could not find delulu-webfetch-mcp binary. "
        "Run `cargo build -p delulu-webfetch-agent --features mcp` first."
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
