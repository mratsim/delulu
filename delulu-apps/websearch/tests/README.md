# Websearch Test Structure

```
tests/
├── README.md                  ← this file
├── fixtures/                  ← zstd-compressed HTML responses for parser tests
│   ├── brave/
│   └── duckduckgo/
├── unit/                      ← pure unit tests (no HTTP, no fixtures)
│   ├── engine_test.rs
│   ├── engines/
│   ├── error_test.rs
│   ├── lib_test.rs
│   ├── mcp_serialization_test.rs
│   ├── parsers_test.rs
│   ├── session_cache_test.rs
│   └── session_key_test.rs
├── integration/               ← fixture-based integration tests (no live HTTP)
│   ├── t_brave_fixtures.rs
│   ├── t_duckduckgo_fixtures.rs
│   └── t_mcp_session.rs
└── manual/                    ← live tests (require network, `#[ignore]`)
    ├── t_websearch_ddg_live.rs
    ├── t_websearch_ddg_next_page_live.rs
    ├── t_websearch_brv_live.rs
    ├── t_websearch_brv_next_page_live.rs
    ├── t_websearch_live.rs
    ├── t_websearch_next_page_live.rs
    └── end-to-end/            ← MCP protocol end-to-end tests
        ├── t_websearch_mcp.rs         ← Rust test harness (smoke + e2e orchestrator)
        ├── mcp_helpers.rs             ← Rust helpers (find_binary, spawn_stdio, read_json)
        ├── websearch_test_utils.py    ← Python helpers + 5 shared test functions
        ├── test_websearch_stdio.py    ← stdio transport tests
        ├── test_websearch_http.py     ← HTTP transport tests
        ├── pyproject.toml             ← Python deps (`mcp>=1.0`)
        └── __init__.py                ← package marker
```

## Running Tests

```bash
# All non-live tests (default)
cargo test -p delulu-websearch --features mcp

# MCP smoke tests only (fast, no network)
cargo test -p delulu-websearch --features mcp --test t_websearch_mcp

# MCP end-to-end tests (live, requires network + Python deps)
cd delulu-apps/websearch/tests/manual/end-to-end && uv sync
cargo test -p delulu-websearch --features mcp --test t_websearch_mcp -- --ignored --nocapture

# Run Python scripts directly
cd delulu-apps/websearch/tests/manual/end-to-end
uv run python3 test_websearch_stdio.py <path-to-binary>
uv run python3 test_websearch_http.py <path-to-binary>
```

## Test Layers

| Layer | Location | Network | Speed | CI |
|-------|----------|---------|-------|----|
| Unit | `tests/unit/` | No | Instant | Yes |
| Integration | `tests/integration/` | No | Fast | Yes |
| Manual live | `tests/manual/` | Yes | Slow | `#[ignore]` |
| MCP e2e | `tests/manual/end-to-end/` | Yes | Slow | `#[ignore]` |
