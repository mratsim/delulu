#!/usr/bin/env python3
import json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
from mcp.client.session import ClientSession
from mcp.client.streamable_http import streamable_http_client

async def run_tests(mcp_port, config_path):
    with open(config_path) as f:
        config = json.load(f)
    url = f"http://127.0.0.1:{mcp_port}/mcp"
    async with streamable_http_client(url) as (r, w, sid):
        async with ClientSession(r, w) as session:
            r = await session.initialize()
            assert r.protocolVersion == "2025-03-26"
            print("  Initialized")
            tools = await session.list_tools()
            names = [t.name for t in tools.tools]
            assert "list_recent_papers" in names
            print(f"  Tools: {names}")
            r = await session.call_tool("list_recent_papers", {})
            assert len(r.content) > 0
            print("  list_recent_papers OK")
    return 0

if __name__ == "__main__":
    import asyncio
    sys.exit(asyncio.run(run_tests(int(sys.argv[1]), sys.argv[2])))
