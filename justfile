default:
	@just --list

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

# Build every shipped binary.
build:
	cargo build --bins

build-heddle:
	cargo build --bin heddle

build-headless:
	cargo build --bin heddle-headless

build-eval:
	cargo build --release --bin eval

build-export-schemas:
	cargo build --bin export-schemas

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

eval evals="evals" prompts="all" tasks="all" model="openrouter/free": build-eval
	./target/release/eval run --evals {{evals}} --prompts {{prompts}} --tasks {{tasks}} --model {{model}}

# Run all tasks for a prompt selection. Use `just eval` for a custom eval path
# or explicit task IDs; eval fixtures do not have grouping/category filters yet.
run-evals prompts="all" model="openrouter/free": build-eval
	./target/release/eval run --prompts {{prompts}} --tasks all --model {{model}}

# Instruction-only matrix: excludes cwd/date/git/file-tree prompt context.
run-evals-static prompts="all" model="openrouter/free": build-eval
	./target/release/eval run --prompts {{prompts}} --tasks all --model {{model}} --static-context-only

clippy:
	cargo clippy --all-targets

check:
	cargo fmt --check
	cargo clippy --all-targets
	cargo test
