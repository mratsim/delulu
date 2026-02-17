#############################################
#                                           #
#           Stage 0: Chef                  #
#                                           #
#############################################
FROM rust:1.92-slim AS chef
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
FROM debian:bookworm-slim

# Install ca-certificates to allow HTTPS call
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m appuser

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /app/target/release/delulu-travel-mcp /app/delulu-travel-mcp

# Switch to the non-root user
USER appuser

# Expose the default port
EXPOSE 8080

# Set the entry point
ENTRYPOINT ["/app/delulu-travel-mcp"]
