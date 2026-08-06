#  Delulu — MCP tools for LLM research
#
#  Copyright (C) 2026  Mamy Ratsimbazafy
#
#  This program is free software: you can redistribute it and/or modify
#  it under the terms of the GNU Affero General Public License as published by
#  the Free Software Foundation, either version 3 of the License, or
#  (at your option) any later version.
#
#  This program is distributed in the hope that it will be useful,
#  but WITHOUT ANY WARRANTY; without even the implied warranty of
#  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
#  GNU Affero General Public License for more details.
#
#  You should have received a copy of the GNU Affero General Public License
#  along with this program.  If not, see <http://www.gnu.org/licenses/>.
#
#############################################
#                                           #
#           Stage 0: Chef                  #
#                                           #
#############################################
FROM rust:1.95-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

#############################################
#                                           #
#           Stage 1: Planner              #
#                                           #
#############################################
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

#############################################
#                                           #
#           Stage 2: Builder             #
#                                           #
#############################################
FROM chef AS builder

# Install build dependencies
# - wreq uses BoringSSL for TLS; boring-sys2 builds it from source by default
# - build-essential: g++, make, etc.
# - cmake: required to build BoringSSL
# - libclang-dev: required for bindgen (boring-sys2 build dependency)
# - git: clone BoringSSL source
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        build-essential cmake libclang-dev git && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy recipe from planner - this layer is cached until recipe.json changes
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --features mcp --recipe-path recipe.json

# Copy source code - this layer will rebuild on source change
COPY . .

# Build the MCP binary
RUN cargo build --release --features mcp --bin delulu-travel-mcp

#############################################
#                                           #
#           Stage 3: Runtime                #
#                                           #
#############################################
FROM debian:bookworm-slim AS delulu-travel-mcp-runtime

# Install ca-certificates for HTTPS root CAs
# Note: libssl3 is not needed - BoringSSL is statically linked via boring-sys2
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m appuser

WORKDIR /app

# Copy the binary from the builder stage (owned by non-root user)
COPY --chown=appuser:appuser --from=builder /app/target/release/delulu-travel-mcp /app/delulu-travel-mcp

# Switch to the non-root user
USER appuser

# Expose the default port
EXPOSE 8080

# Set the entry point
ENTRYPOINT ["/app/delulu-travel-mcp"]

#############################################
#                                           #
#        Stage 4: all-mcp Builder          #
#                                           #
#############################################
FROM builder AS delulu-all-mcp-builder

# Build the all-mcp binary (BoringSSL needs cmake/libclang from the builder stage)
RUN cargo build --release --features mcp --bin delulu-all-mcp

#############################################
#                                           #
#        Stage 5: all-mcp Runtime          #
#                                           #
#############################################
FROM debian:bookworm-slim AS delulu-all-mcp-runtime

# Install ca-certificates for HTTPS root CAs
# Note: libssl3 is not needed - BoringSSL is statically linked via boring-sys2
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m appuser

WORKDIR /app

# Copy the binary from the all-mcp builder stage (owned by non-root user)
COPY --chown=appuser:appuser --from=delulu-all-mcp-builder /app/target/release/delulu-all-mcp /app/delulu-all-mcp

# Switch to the non-root user
USER appuser

# Expose the default port
EXPOSE 8080

# Set the entry point
ENTRYPOINT ["/app/delulu-all-mcp"]