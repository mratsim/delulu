#############################################
#                                           #
#           Stage 1: Builder                #
#                                           #
#############################################
FROM rust:1.92-slim AS builder

# Install dependencies needed for building reqwest (native-tls)
RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config libssl-dev protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY delulu-apps/travel-agent/Cargo.toml delulu-apps/travel-agent/
COPY delulu-internals/query-queues/Cargo.toml delulu-internals/query-queues/

# Build dependencies to cache them
RUN mkdir delulu-apps/travel-agent/src && \
    mkdir delulu-internals/query-queues/src && \
    echo 'fn main() {}' > delulu-apps/travel-agent/src/main.rs && \
    echo '' > delulu-internals/query-queues/src/lib.rs && \
    cargo build --release -p delulu-travel-mcp --features mcp && \
    rm -rf delulu-apps/travel-agent/src delulu-internals/query-queues/src

# Copy the actual source code
COPY delulu-apps/travel-agent/src delulu-apps/travel-agent/src
COPY delulu-internals/query-queues/src delulu-internals/query-queues/src
# Unneeded, we hardcode the Protobuf generated code in the repo
# COPY delulu-apps/travel-agent/build.rs delulu-apps/travel-agent/build.rs

# Build the application, this will use the cached dependencies
# We update the timestamp to force a cargo rebuild with actual code
# Or we might end up with an empty application.
RUN touch delulu-apps/travel-agent/src/main_mcp.rs && \
    cargo build --release -p delulu-travel-mcp --features mcp

#############################################
#                                           #
#           Stage 2: Runtime                #
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
