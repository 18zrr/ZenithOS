# Makefile for Zenith UI components

.PHONY: all build release clean test fmt check

all: build

build:
	cargo build --manifest-path zenith-ui/Cargo.toml

release:
	cargo build --release --manifest-path zenith-ui/Cargo.toml

clean:
	cargo clean --manifest-path zenith-ui/Cargo.toml

test:
	cargo test --manifest-path zenith-ui/Cargo.toml

fmt:
	cargo fmt --manifest-path zenith-ui/Cargo.toml

check:
	cargo check --manifest-path zenith-ui/Cargo.toml
