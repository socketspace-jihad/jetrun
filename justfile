default:
    @just --list

# Build all Rust services in debug mode
build:
    cargo build --workspace

# Build all Rust services in release mode (OS-optimized)
build-release:
    RUSTFLAGS="-C target-cpu=native" cargo build --workspace --release

# Run all tests
test:
    cargo test --workspace

# Run clippy
lint:
    cargo clippy --workspace -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Start gateway service (dev)
run-gateway:
    cargo run --bin jetrun-gateway

# Start engine service (dev)
run-engine:
    cargo run --bin jetrun-engine

# Start worker service (dev)
run-worker:
    cargo run --bin jetrun-worker

# Start cache service (dev)
run-cache:
    cargo run --bin jetrun-cache

# Start frontend dev server
run-web:
    cd web && npm run dev

# Install frontend dependencies
web-install:
    cd web && npm install

# Docker compose up (all services)
up:
    docker compose up -d

# Docker compose down
down:
    docker compose down

# Docker compose build
docker-build:
    docker compose build

# Clean build artifacts
clean:
    cargo clean
    cd web && rm -rf .next node_modules
