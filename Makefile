SHELL := /usr/bin/env bash

.PHONY: fmt fmt-check test lint panel-build e2e check ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

test:
	cargo test -q

lint:
	cargo clippy --all-targets --all-features -- -D warnings

panel-build:
	cd panel && npm ci && npm run build

e2e:
	./scripts/e2e-agent-flow.sh

check: fmt-check lint test panel-build e2e

ci: check
