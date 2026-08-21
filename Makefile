.PHONY: check test run release fmt lint notices

# 'help' uses inline ## for descriptions, and ##@ for sections
help: ## Show the available targets.
	@awk 'BEGIN {FS = ":.*## "}; /^##@ / {printf "\n  ----- %s -----\n", substr($$0, 5)}; /^[a-zA-Z0-9_.%-]+:.*## / {printf "  %-20s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

run: ## Run the application. Optionally use `LOG=1` to write to ./logs
ifdef LOG
	@mkdir -p logs
	MEDIA_RIP_LOG=debug MEDIA_RIP_EPISODE_REPORT_DIR=logs cargo run -p media-rip 2>logs/$$(date -Iseconds).log
else
	cargo run -p media-rip
endif

release:
	cargo build --release -p media-rip

notices: ## Regenerate third-party Rust dependency notices with cargo-about.
	cargo about generate --config about.toml --workspace --locked --fail --output-file THIRD_PARTY_NOTICES.txt packaging/third-party-notices.hbs

check: ## Check for any errors
	cargo check --workspace

test: ## Run the application tests
	cargo test --workspace

fmt: ## Run the code formatter
	cargo fmt --all

lint: ## Check for linting warnings
	cargo clippy --workspace --all-targets -- -D warnings
