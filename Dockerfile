# syntax=docker/dockerfile:1

# Cloudflare Containers run on amd64, so default to that platform.
# Build with: docker build --platform linux/amd64 -t drill .
ARG TARGETPLATFORM=linux/amd64

# Build stage
FROM rust:1.88-slim-bullseye AS builder

WORKDIR /usr/src/app

# Install build dependencies. aws-lc-sys (pulled in by rustls) needs cmake.
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy the workspace manifests and source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY postman2drill ./postman2drill

# Build both binaries (drill + postman2drill)
RUN cargo build --release --workspace --locked

# Sanity check
RUN ./target/release/drill --version && ./target/release/postman2drill --version

# Runtime stage using Ubuntu 22.04 LTS
FROM ubuntu:22.04

# Install ca-certificates so drill can make HTTPS requests.
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the compiled binaries into PATH
COPY --from=builder /usr/src/app/target/release/drill /usr/local/bin/drill
COPY --from=builder /usr/src/app/target/release/postman2drill /usr/local/bin/postman2drill

# Keep the container alive by default so it can be used as a long-running service.
CMD ["tail", "-f", "/dev/null"]
