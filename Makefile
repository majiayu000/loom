SHELL := /usr/bin/env bash

.PHONY: fmt fmt-check test lint panel-build panel-test panel-typecheck e2e perf-smoke check ci install-hooks

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

install-hooks:
	git config core.hooksPath .githooks
	@echo "pre-commit hook installed (core.hooksPath=.githooks). Disable with 'git config --unset core.hooksPath'."

test:
	cargo test -q

lint:
	cargo clippy --all-targets --all-features -- -D warnings

panel-build:
	cd panel && bun install --frozen-lockfile && bun run build

panel-test:
	cd panel && bun install --frozen-lockfile && bun run test

panel-typecheck:
	cd panel && bun install --frozen-lockfile && bun run typecheck

e2e:
	./scripts/e2e-agent-flow.sh

perf-smoke:
	cargo build --release --locked
	./scripts/perf-smoke.sh

check: fmt-check lint test panel-typecheck panel-test panel-build e2e perf-smoke

ci: check
