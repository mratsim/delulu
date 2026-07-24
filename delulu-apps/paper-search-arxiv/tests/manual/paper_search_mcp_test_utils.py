#!/usr/bin/env python3
"""Shared test utilities for paper-search MCP integration tests."""

import socket
import sys
import time
from pathlib import Path


def find_server_binary(binary_name: str) -> Path:
    """Find the MCP server binary by walking up to workspace root."""
    tests_dir = Path(__file__).parent.resolve()
    # Walk up to find the delulu/ workspace root
    workspace = tests_dir
    while workspace.name != "delulu" and workspace.parent != workspace:
        workspace = workspace.parent
    if workspace.name != "delulu":
        # Try going up from the tests directory
        workspace = tests_dir.parent.parent.parent.parent
    for candidate in [
        workspace / "target" / "debug" / binary_name,
        workspace / "target" / "release" / binary_name,
    ]:
        if candidate.exists():
            return candidate
    # On Windows, also check with .exe extension
    if sys.platform == "win32":
        for candidate in [
            workspace / "target" / "debug" / f"{binary_name}.exe",
            workspace / "target" / "release" / f"{binary_name}.exe",
        ]:
            if candidate.exists():
                return candidate
    raise RuntimeError(
        f"Could not find {binary_name} binary. "
        f"Run `cargo build -p <crate> --features mcp` first."
    )


def wait_for_server(port: int, timeout: float = 5.0) -> None:
    """Poll until server is ready to accept connections."""
    start = time.time()
    while time.time() - start < timeout:
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.settimeout(0.1)
                s.connect(("127.0.0.1", port))
            return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError(f"Server not ready on port {port}")
