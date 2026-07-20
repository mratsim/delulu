#!/usr/bin/env python3
"""End-to-end integration test: serve fixture, run CLI, validate output structure."""

import json
import os
import re
import signal
import socket
import subprocess
import sys
import time
import zstandard
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler
from threading import Thread

HERE = Path(__file__).parent
FIXTURES = HERE / "fixtures-webfetch"
WORKSPACE = HERE.parent.parent.parent  # delulu/
CLI_BINARY = WORKSPACE / "target" / "release" / "delulu-fetch"
FIXTURE_HTML = "dankrad-pcs-multiproofs.html.zst"


def find_free_port():
    """Return a free TCP port on 127.0.0.1."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class FixtureHandler(BaseHTTPRequestHandler):
    fixture_data: bytes = b""

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(self.fixture_data)))
        self.end_headers()
        self.wfile.write(self.fixture_data)

    def log_message(self, fmt, *args):
        pass


def load_zst(path: Path, max_size: int = 10_000_000) -> bytes:
    """Decompress a .zst fixture file."""
    with open(path, "rb") as f:
        compressed = f.read()
    dctx = zstandard.ZstdDecompressor()
    return dctx.decompress(compressed, max_output_size=max_size)


def count_markdown_structure(md: str) -> dict:
    """Count structural elements in Markdown output."""
    return {
        "headers": len(re.findall(r"^#{1,6}\s", md, re.MULTILINE)),
        "paragraphs": len(
            [
                p
                for p in md.split("\n\n")
                if p.strip()
                and not p.startswith("#")
                and not p.startswith("---")
                and not p.startswith("![")
                and not p.startswith("[")
            ]
        ),
        "links": len(re.findall(r"\[([^\]]+)\]\(([^)]+)\)", md)),
        "images": len(re.findall(r"!\[([^\]]*)\]\(([^)]+)\)", md)),
        "code_blocks": len(re.findall(r"```", md)) // 2,
        "total_chars": len(md),
    }


def test_end_to_end() -> bool:
    """Run the end-to-end test. Returns True on success, False on failure."""
    # --- Pre-flight checks ---------------------------------------------------
    if not CLI_BINARY.is_file():
        print(f"FAIL: CLI binary not found at {CLI_BINARY}", file=sys.stderr)
        return False

    html_path = FIXTURES / FIXTURE_HTML
    if not html_path.is_file():
        print(f"FAIL: Fixture not found at {html_path}", file=sys.stderr)
        return False

    # Load fixture
    try:
        html = load_zst(html_path)
    except Exception as exc:
        print(f"FAIL: Could not load fixture: {exc}", file=sys.stderr)
        return False

    print(f"INFO: Loaded HTML ({len(html)} bytes)")

    # --- Start HTTP server ---------------------------------------------------
    port = find_free_port()
    FixtureHandler.fixture_data = html

    server = HTTPServer(("127.0.0.1", port), FixtureHandler)
    server_thread = Thread(target=server.serve_forever, daemon=True)
    server_thread.start()
    time.sleep(0.2)

    url = f"http://127.0.0.1:{port}/"

    # --- Run CLI -------------------------------------------------------------
    try:
        result = subprocess.run(
            [str(CLI_BINARY), "-u", url],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired:
        print("FAIL: CLI timed out after 30 seconds", file=sys.stderr)
        return False
    finally:
        server.shutdown()

    # --- Structural assertions -----------------------------------------------
    if result.returncode != 0:
        print(f"FAIL: CLI exited with code {result.returncode}", file=sys.stderr)
        print(f"stderr: {result.stderr[:500]}", file=sys.stderr)
        return False

    output = result.stdout

    # Must have YAML frontmatter
    if not output.startswith("---\n"):
        print("FAIL: Output does not start with YAML frontmatter '---'", file=sys.stderr)
        print(f"First 300 chars: {output[:300]}", file=sys.stderr)
        return False

    # Extract body (skip frontmatter)
    if "---\n\n" not in output:
        print("FAIL: Output missing closing frontmatter delimiter", file=sys.stderr)
        return False
    body = output.split("---\n\n", 1)[1]

    # Structural sanity checks
    actual = count_markdown_structure(body)

    checks = [
        ("body_length > 500", len(body) > 500),
        ("has_heading", actual["headers"] >= 1),
        ("has_paragraph", actual["paragraphs"] >= 1),
        ("total_chars_range", 1000 < len(body) < 50000),
    ]

    all_pass = True
    for name, ok in checks:
        status = "PASS" if ok else "FAIL"
        if not ok:
            all_pass = False
        print(f"  [{status}] {name}")

    if all_pass:
        print(f"\nPASS: {url} -> {len(body)} chars body, structural sanity checks passed")
    else:
        print(f"\nFAIL: structural sanity check failed", file=sys.stderr)

    return all_pass


def main():
    success = test_end_to_end()
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
