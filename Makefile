SHELL := /usr/bin/env bash

PANEL_DIR := panel
PANEL_INSTALL_STAMP := $(PANEL_DIR)/node_modules/.bun-install.stamp

.PHONY: fmt fmt-check test lint panel-install panel-dev panel-build panel-test panel-typecheck e2e perf-smoke check ci install-hooks

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

install-hooks:
	git config core.hooksPath .githooks
	@echo "pre-commit hook installed (core.hooksPath=.githooks). Disable with 'git config --unset core.hooksPath'."

test:
	cargo nextest run --no-fail-fast

lint:
	cargo clippy --all-targets --all-features -- -D warnings

$(PANEL_INSTALL_STAMP): $(PANEL_DIR)/package.json $(PANEL_DIR)/bun.lock
	cd $(PANEL_DIR) && bun install --frozen-lockfile
	mkdir -p $(dir $@)
	touch $@

panel-install: $(PANEL_INSTALL_STAMP)

panel-dev: panel-install
	cd $(PANEL_DIR) && bun run dev

panel-build: panel-install
	cd $(PANEL_DIR) && bun run build

panel-test: panel-install
	cd $(PANEL_DIR) && bun run test

panel-typecheck: panel-install
	cd $(PANEL_DIR) && bun run typecheck

e2e:
	./scripts/e2e-agent-flow.sh

perf-smoke: panel-build
	cargo build --release --locked
	./scripts/perf-smoke.sh

check: fmt-check lint test panel-typecheck panel-test panel-build e2e perf-smoke

ci: check
