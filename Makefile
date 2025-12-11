# ABOUTME: Makefile for gorp - Matrix-Claude bridge
# ABOUTME: Common development tasks: build, test, run, release, docker

.PHONY: build run release test clean docker docker-up docker-down fmt lint check watch dev-deps help

# Default target
help:
	@echo "gorp - Matrix-Claude Bridge"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build      Build debug binary"
	@echo "  release    Build optimized release binary"
	@echo "  run        Run in debug mode"
	@echo "  test       Run all tests"
	@echo "  fmt        Format code"
	@echo "  lint       Run clippy lints"
	@echo "  check      Run fmt + lint + test"
	@echo "  clean      Remove build artifacts"
	@echo "  docker     Build Docker image"
	@echo "  docker-up  Run in Docker (detached)"
	@echo "  docker-down Stop Docker"
	@echo ""
	@echo "Release: Use GitHub Actions (Actions → Release → Run workflow)"
	@echo ""

# Build debug binary
build:
	cargo build

# Build release binary
release:
	cargo build --release

# Run in debug mode
run:
	cargo run

# Run release binary
run-release: release
	./target/release/gorp

# Run all tests
test:
	cargo test

# Format code
fmt:
	cargo fmt

# Run clippy lints
lint:
	cargo clippy -- -D warnings

# Full check: format, lint, test
check: fmt lint test

# Clean build artifacts
clean:
	cargo clean

# Build Docker image
docker:
	docker build -t gorp:latest .

# Run in Docker (detached)
docker-up:
	docker-compose up -d

# Stop Docker
docker-down:
	docker-compose down

# Watch for changes and rebuild (requires cargo-watch)
watch:
	cargo watch -x build

# Install development dependencies
dev-deps:
	cargo install cargo-watch
