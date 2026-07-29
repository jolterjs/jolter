.PHONY: all build build-release test check fmt fmt-check format clippy lint coverage audit run exec help

.DEFAULT_GOAL := help

help:
	@echo "Available make targets:"
	@echo "  make install        - Install dependencies"
	@echo "  make build          - Build workspace"
	@echo "  make build-release  - Build workspace for release"
	@echo "  make test           - Run all tests"
	@echo "  make fmt            - Format code (prettier + cargo fmt)"
	@echo "  make fmt-check      - Check code formatting"
	@echo "  make clippy         - Run cargo clippy"
	@echo "  make lint           - Alias for clippy"
	@echo "  make coverage       - Run test coverage"
	@echo "  make audit          - Run cargo audit"
	@echo "  make run ARGS=\"...\" - Run Jolter binary with ARGS"

all: install fmt-check clippy test build

install:
	bun install

build:
	cargo build --workspace --all-targets --locked $(ARGS)

build-release:
	cargo build --release --locked $(ARGS)

test:
	cargo test --workspace --all-targets --locked

fmt:
	bun concurrently "prettier --write ." "cargo fmt --all"

format: fmt

fmt-check:
	bun prettier --check .
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets --locked -- -D warnings

lint: clippy

coverage:
	cargo llvm-cov --workspace --all-targets --locked --fail-under-lines 80

audit:
	cargo audit

run:
	cargo run -p jolter-cli --bin jolter -- $(ARGS)

exec: run
