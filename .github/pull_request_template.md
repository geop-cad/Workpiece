## What this changes

## Why

## How it was checked
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `bench/boolean` run, if `lib/boolean`, `ops/step` or the blends changed
- [ ] A test for the fix or the new feature

## Notes
- [ ] No STEP/IGES files or large binaries added
- [ ] I wrote this, or I may license it under MIT OR Apache-2.0
