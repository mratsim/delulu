# End-to-End Integration Tests

These tests validate mcpify's MCP protocol handling by running real backend services,
proxying them through mcpify, and exercising the full MCP lifecycle
(initialize → list_tools → call_tool) over both HTTP and stdio transports.

## Architecture

```
┌──────────────┐     HTTP      ┌──────────────────┐      ┌─────────────┐
│  Service A   │◄──────────────┤  mcpify instance  │      │             │
│  (axum)      │   (proxy)     │  A                │◄────►│  Python     │
│  :3001       │──────────────►│  spec_a.json      │      │  test       │
└──────────────┘               └──────────────────┘      │  script     │
                                                         │  (mcp SDK)  │
┌──────────────┐     HTTP      ┌──────────────────┐      │             │
│  Service B   │◄──────────────┤  mcpify instance  │      │             │
│  (axum)      │   (proxy)     │  B                │◄────►│             │
│  :3002       │──────────────►│  spec_b.json      │      └─────────────┘
└──────────────┘               └──────────────────┘
```

Two transport modes are tested:
- **HTTP** (`test_mcp_http.py`): connects via `streamable_http_client(url)`
- **Stdio** (`test_mcp_stdio.py`): spawns mcpify via `stdio_client(StdioServerParameters)`

## Test flow

1. **Rust test harness** starts the infrastructure:
   - Two axum backend services (service A: `/users`, service B: `/items`)
   - Writes OpenAPI spec files with the assigned ports
   - For HTTP: starts two mcpify instances with `http` transport
   - For stdio: passes spec file paths to the Python script

2. **Python test script** exercises the MCP protocol:
   - Uses the well-validated `mcp` Python SDK for all protocol handling
   - Runs initialize → list_tools → call_tool against each service
   - Verifies responses match expected backend data exactly

3. **Cleanup**: Rust test shuts down all services and processes.

## Files

| File | Role |
|------|------|
| `http_test.rs` | Rust orchestrator for HTTP transport test (also holds http-only helpers `get_free_port`, `stream_stderr_to_console`) |
| `stdio_test.rs` | Rust orchestrator for stdio transport test |
| `helpers.rs` | Shared Rust helpers: `find_binary`, `write_spec`, `health_check`, `init_tracing`, `E2eGuard` |
| `service_a.rs` | Axum app: `GET /users` → `[{"id":1,"name":"Alice"}]`, `GET /health` |
| `service_b.rs` | Axum app: `GET /items` → `[{"id":1,"item":"Widget"}]`, `GET /health` |
| `spec_a.json` | OpenAPI 3.0 spec for service A (uses `{PORT}` placeholder) |
| `spec_b.json` | OpenAPI 3.0 spec for service B (uses `{PORT}` placeholder) |
| `test_mcp_http.py` | Python test script for HTTP transport |
| `test_mcp_stdio.py` | Python test script for stdio transport |
| `mcp_e2e_utils.py` | Python helpers: `find_binary`, `wait_for_port`, `kill_process` |
| `pyproject.toml` | Python project config (dependency: `mcp>=1.0.0`) |

## Running

```bash
# Install Python dependencies
cd delulu-apps/mcpify/tests/integration/end_to_end
python3 -m pip install -e .

# Build mcpify with MCP feature
cargo build -p delulu-mcpify --features mcp

# Run all tests
cargo test -p delulu-mcpify --features mcp

# Run only the HTTP e2e test
cargo test -p delulu-mcpify --features mcp --test mcpify_e2e_http

# Run only the stdio e2e test
cargo test -p delulu-mcpify --features mcp --test mcpify_e2e_stdio
```

## Why Python for the MCP protocol?

The Python `mcp` SDK (`mcp.client.session.ClientSession`, `streamable_http_client`,
`stdio_client`) handles all MCP wire-format details: SSE event parsing, chunked
transfer encoding, JSON-RPC framing, session management, and protocol version
negotiation. Using it avoids maintaining ad-hoc raw TCP/HTTP parsing code in
Rust tests, and the SDK is well-validated across many projects.
