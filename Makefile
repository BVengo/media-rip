.PHONY: check test run release fmt lint

check:
	cargo check --workspace

test:
	cargo test --workspace

run:
	cargo run -p media-rip

release:
	cargo build --release -p media-rip

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

