# Signal Report: QUAL-01 Module Ceiling Regression

## Scope

Restore the 600-line module ceiling in the Rust codebase without changing runtime behavior.

## Evidence

- `wc -l src/panel/skill_diff.rs src/panel/mod.rs src/state/mod.rs`
- `rg -n "#\\[cfg\\(test\\)\\]" src/panel/mod.rs src/panel/skill_diff.rs src/state/mod.rs`
- Direct source inspection of the three flagged modules

## Confirmed Root Cause

1. `src/panel/skill_diff.rs` is 701 lines because the production diff handler and its parsing/git helper tests are co-located in one file. The embedded `#[cfg(test)]` block starts at line 410.
2. `src/panel/mod.rs` is 853 lines because the panel bootstrap/router code and a large mixed test fixture suite are co-located. The embedded `#[cfg(test)]` block starts at line 146.
3. `src/state/mod.rs` is 630 lines because the state/runtime helpers and their lock regression tests are co-located. The embedded `#[cfg(test)]` block starts at line 486.

The regression is structural, not behavioral: test code drifted back into top-level production modules, pushing them past the maintainability ceiling.

## Remediation Plan

1. Keep runtime code in the existing modules.
2. Move each embedded `#[cfg(test)]` block into a dedicated sibling test module:
   - `src/panel/tests.rs`
   - `src/panel/skill_diff/tests.rs`
   - `src/state/tests.rs`
3. Preserve existing coverage and imports.
4. Validate with `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
