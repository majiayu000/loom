SHELL := /usr/bin/env bash

PANEL_DIR := panel
PANEL_INSTALL_STAMP := $(PANEL_DIR)/node_modules/.bun-install.stamp
LOOM_CONTRACT_DIFF_BASE ?= $(shell git merge-base HEAD origin/main 2>/dev/null)
ifeq ($(shell uname -s),Linux)
LOOM_PERF_RUSTFLAGS ?= -Cllvm-args=-enable-machine-outliner=always -Clink-arg=-Wl,--no-eh-frame-hdr
else
LOOM_PERF_RUSTFLAGS ?= -Cllvm-args=-enable-machine-outliner=always
endif

.PHONY: fmt fmt-check test contract-policy lint module-ceiling module-ceiling-test panel-install panel-dev panel-build panel-test panel-typecheck e2e perf-smoke check ci install-hooks

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

install-hooks:
	git config core.hooksPath .githooks
	@echo "pre-commit hook installed (core.hooksPath=.githooks). Disable with 'git config --unset core.hooksPath'."

test:
	./scripts/test-with-contract-trace.sh

contract-policy:
	test -n "$(LOOM_CONTRACT_DIFF_BASE)"
	git cat-file -e "$(LOOM_CONTRACT_DIFF_BASE)^{tree}"
	LOOM_CONTRACT_DIFF_BASE="$(LOOM_CONTRACT_DIFF_BASE)" cargo test --test agent_contract_surfaces contract_range_policy_current_diff -- --exact

lint:
	cargo clippy --all-targets --all-features -- -D warnings

module-ceiling:
	./scripts/module-ceiling.sh

module-ceiling-test:
	./scripts/module-ceiling-test.sh

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
	RUSTFLAGS="$(LOOM_PERF_RUSTFLAGS) $${RUSTFLAGS:-}" cargo build --release --locked
	./scripts/perf-smoke.sh

check: fmt-check lint module-ceiling module-ceiling-test test contract-policy panel-typecheck panel-test panel-build e2e perf-smoke

ci: check
