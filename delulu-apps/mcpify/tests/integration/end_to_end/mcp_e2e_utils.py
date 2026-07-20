#!/usr/bin/env python3
"""Shared test utilities for mcpify end-to-end tests."""

import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


def find_binary() -> Path:
    """Find the compiled mcpify binary using CARGO_MANIFEST_DIR."""
    manifest = os.environ.get("CARGO_MANIFEST_DIR")
    if manifest:
        root = Path(manifest).resolve()
    else:
        # Fallback: walk up from this file
        root = Path(__file__).resolve().parent.parent.parent.parent.parent
    for path in [
        root / "target" / "debug" / "mcpify",
        root / "target" / "release" / "mcpify",
    ]:
        if path.exists():
            return path
    raise RuntimeError(
        "mcpify binary not found. Run `cargo build -p delulu-mcpify --features mcp` first."
    )


def wait_for_port(port: int, timeout: float = 10.0) -> None:
    """Poll until a TCP port is accepting connections."""
    start = time.time()
    while time.time() - start < timeout:
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(0.2)
            s.connect(("127.0.0.1", port))
            s.close()
            return
        except (ConnectionRefusedError, OSError):
            time.sleep(0.1)
    raise RuntimeError(f"Port {port} not ready after {timeout}s")


def kill_process(child: subprocess.Popen) -> None:
    """Kill a process and its process group."""
    if sys.platform != "win32":
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except (ProcessLookupError, OSError):
            pass
    child.terminate()
    try:
        child.wait(timeout=3)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait()
