# Delulu - A suite of MCP tools to supercharge your LLM skillset

> [!TIP]
> _More knowing. Less guessing._

Delulu will provide a suite of tools to help your LLM search and crawl the Web and API
to give you high-quality information.

The first tool released is a travel agent that can search on:
- Google Flights
- Google Hotels

## Motivation

> [!TIP]
> One chat UI to rule them all\
> One MCP to find them\
> One query to bring them all\
> And by the LLM bind them

## Screenshots

![delulu-flights-search.png](./media/delulu-flights-search.png)
![delulu-hotels-search.png](./media/delulu-hotels-search.png)

## Features

- 🪄 **MCP Server:** Works with stdio transport (Claude Desktop) and HTTP transport
- 💻 **CLI Tools:** Direct `flights` and `hotels` commands
- 🚦 **Rate Limiting:** Let's be good citizens.
- 🍪 **Cookie Management:** Let's be crafty citizens.
- 📦 **Prebuilt Binaries:** For those allergic to Docker
- 🐳 **Container Ready:** Docker, Podman and Docker-Compose support with prebuilt image
- 🚀 **High Performance:** Built with Rust, Axum, and Tokio
- ☁️ **Lightweight:** No browser, Selenium, or Playwright - direct queries via reverse-engineered Protobuf

## Installation

### 📦 Option 1: Prebuilt Binaries

1. **Quick Install** - Platform-specific one-liners:

<details>
   <summary>Windows (x86-64 i.e. AMD or Intel)</summary>

   ```powershell
   Invoke-WebRequest -Uri "https://github.com/mratsim/delulu/releases/latest/download/delulu-travel-mcp-windows-x86_64.zip" -OutFile "delulu.zip"; Expand-Archive -Path "delulu.zip" -DestinationPath "."; Remove-Item "delulu.zip"
   ```
   </details>

   <details>
   <summary>Windows ARM64</summary>

   ```powershell
   Invoke-WebRequest -Uri "https://github.com/mratsim/delulu/releases/latest/download/delulu-travel-mcp-windows-arm64.zip" -OutFile "delulu.zip"; Expand-Archive -Path "delulu.zip" -DestinationPath "."; Remove-Item "delulu.zip"
   ```
   </details>

   <details>
   <summary>Linux (x86-64 i.e. AMD or Intel)</summary>

   ```bash
   curl -sL "https://github.com/mratsim/delulu/releases/latest/download/delulu-travel-mcp-linux-x86_64.tar.gz" | tar -xz
   ```
   </details>

   <details>
   <summary>Linux (Arm64 like Raspberry Pi)</summary>

   ```bash
   curl -sL "https://github.com/mratsim/delulu/releases/latest/download/delulu-travel-mcp-linux-arm64.tar.gz" | tar -xz
   ```
   </details>

   <details>
   <summary>macOS Apple Silicon</summary>

   ```bash
   curl -sL "https://github.com/mratsim/delulu/releases/latest/download/delulu-travel-mcp-macos-arm64.tar.gz" | tar -xz
   ```
   </details>

2. **Manual Download** - Visit the [GitHub Releases page](https://github.com/mratsim/delulu/releases) and download the appropriate file for your platform and architecture, then **extract** the binary.

3. **Run** the MCP server:

   ```bash
   # HTTP transport (for remote clients)
   delulu-travel-mcp http --host 0.0.0.0 --port 8080

   # Or stdio transport (for Claude Desktop)
   delulu-travel-mcp stdio
   ```

### 🦀 Option 2: Cargo (Rust Users)

```bash
# Build from source
git clone https://github.com/mratsim/delulu
cd delulu
cargo build --release --features mcp

# Run the MCP server
./target/release/delulu-travel-mcp http --port 8080
```

### 🐳 Option 3: Docker or Podman

```bash
# HTTP transport (for remote clients)
docker run -p 8080:8080 \
  --env RUST_LOG=info \
  ghcr.io/mratsim/delulu/travel-agent:latest \
  http --host 0.0.0.0 --port 8080
```

```bash
# Stdio transport (for Claude Desktop) - requires interactive mode
docker run -i --rm ghcr.io/mratsim/delulu/travel-agent:latest stdio
```

### 🐳 Option 4: Docker-compose

```bash
git clone https://github.com/mratsim/delulu
cd delulu
docker-compose up -d
```

The docker-compose uses HTTP transport. Access the MCP server at `http://localhost:8080/mcp`.

## Usage

### CLI Tools

Search for flights:

```bash
delulu-flights --from JFK --to LAX --date 2026-03-15 --seat economy --adults 1
```

Search for hotels:

```bash
delulu-hotels --location "San Francisco, CA" --date 2026-03-15 --adults 2
```

### MCP Server

#### HTTP Transport

Point your MCP client to `http://localhost:8080/mcp`.

Available tools:
- `flights_search`: Search flights prices on Google Flights
- `hotels_search`: Search hotel prices on Google Hotels

#### Stdio Transport (Claude Desktop)

Add to your Claude Desktop config:

```json
{
  "mcpServers": {
    "delulu": {
      "command": "delulu-travel-mcp",
      "args": ["stdio"]
    }
  }
}
```

## License

Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).

This program is free software: you can redistribute it and/or modify it under the terms of the GNU Affero General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See [LICENSE](LICENSE) or <http://www.gnu.org/licenses/> for details.
