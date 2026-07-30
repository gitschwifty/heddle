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

eval model="openrouter/free" evals="evals" prompts="all" tasks="all" tag="": build-eval
	./target/release/eval run --evals {{evals}} --prompts {{prompts}} --tasks {{tasks}} --model {{model}}{{ if tag != "" { " --tag " + tag } else { "" } }}

# Run the full prompt/task matrix. Example: `just run-evals z-ai/glm-4.7-flash 3 repeat-3`.
run-evals model="openrouter/free" runs="1" tag="": build-eval
	./target/release/eval run --prompts all --tasks all --model {{model}} --runs {{runs}}{{ if tag != "" { " --tag " + tag } else { "" } }}

# Instruction-only matrix. Example: `just run-evals-static z-ai/glm-4.7-flash 3 repeat-3`.
run-evals-static model="openrouter/free" runs="1" tag="": build-eval
	./target/release/eval run --prompts all --tasks all --model {{model}} --runs {{runs}} --static-context-only{{ if tag != "" { " --tag " + tag } else { "" } }}

# Refresh one existing aggregate from all matching retained runs.
aggregate-evals suite="suite" profile="default": build-eval
	./target/release/eval aggregate --suite-label {{suite}} --profile-label {{profile}}

# Create an aggregate from one run. Example: `just create-aggregate default-v1 base evals/results/glm-4.7-flash/<run>`.
create-aggregate suite profile run: build-eval
	./target/release/eval aggregate --suite-label {{suite}} --profile-label {{profile}} --run {{run}}

clippy:
	cargo clippy --all-targets

check:
	cargo fmt --check
	cargo clippy --all-targets
	cargo test
