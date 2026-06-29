default:
	@just --list

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

test:
	cargo test

test-e2e:
	cargo test --test e2e_simple_task

test-provider-live:
	HEDDLE_INTEGRATION_TESTS=1 cargo test --test provider_openrouter_integration -- --nocapture

test-multi-turn-live:
	HEDDLE_INTEGRATION_TESTS=1 HEDDLE_SLOW_TESTS=1 cargo test --test multi_turn_integration -- --nocapture

test-live:
	HEDDLE_INTEGRATION_TESTS=1 HEDDLE_SLOW_TESTS=1 cargo test --test provider_openrouter_integration --test multi_turn_integration -- --nocapture

clippy:
	cargo clippy --all-targets

check:
	cargo fmt --check
	cargo clippy --all-targets
	cargo test
