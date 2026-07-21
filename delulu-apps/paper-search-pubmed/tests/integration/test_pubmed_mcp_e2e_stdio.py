#!/usr/bin/env python3
import json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
from mcp import ClientSession
from mcp.client.stdio import stdio_client, StdioServerParameters

async def run_tests(binary, config_path):
    with open(config_path) as f:
        config = json.load(f)
    params = StdioServerParameters(command=binary, args=["--api-base-url", config["fixture_url"], "stdio"])
    async with stdio_client(params) as (r, w):
        async with ClientSession(r, w) as session:
            r = await session.initialize()
            assert r.protocolVersion == "2025-03-26"
            print("  Initialized")
            tools = await session.list_tools()
            names = [t.name for t in tools.tools]
            assert "search_pubmed" in names
            print(f"  Tools: {names}")
            r = await session.call_tool("get_database_info", {})
            assert len(r.content) > 0
            text = r.content[0].text
            print(f"  Response text: {text[:200]}"); data = json.loads(text)
            assert "db_name" in data
            print("  get_database_info OK")
    return 0

if __name__ == "__main__":
    import asyncio
    sys.exit(asyncio.run(run_tests(sys.argv[1], sys.argv[2])))
