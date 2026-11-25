# Contributing to ShapeCraft

Thank you for looking at ShapeCraft. This page says how to build the project, what a change has to pass, and how to send it. By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Build

You need Rust 1.95 or newer and a C compiler; see the build section of the [README](README.md#build-and-run) for the system libraries the app loads when it runs.

```bash
cargo build --workspace
cargo run --release -p shapecraft-app     # the app; the geometry is slow without the optimizer
```

The `dev` profile already optimizes the dependencies and the geometry a little (see the root `Cargo.toml`), so `cargo test` is usable without `--release`.

## Before you send a change

All three must be clean:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The workspace sets `unsafe_code = "forbid"` (the app crate `deny`s it and allows it only in its OpenGL renderer) and `missing_docs = "warn"`: public items need a doc comment.

If you change `lib/boolean`, the STEP importer (`ops/step`) or the blends (`lib/shape`), also run the boolean benchmark, which is not part of `cargo test`:

```bash
cargo run --release -p shapecraft-boolean-bench -- fetch   # once: downloads the data sets
cargo run --release -p shapecraft-boolean-bench
```

It fails when a case that passed in `bench/boolean/baseline.tsv` no longer does. See [`bench/boolean/README.md`](bench/boolean/README.md).

## How the tests are organised

- Unit tests sit next to the code in `#[cfg(test)]` modules (many in a `tests.rs` beside the file they test).
- Each `ops/*` crate that has integration tests keeps them in its `tests/` folder; they build parts with the operations and check volumes, faces and names. `ops/testing` has the helpers they share (sketches and boxes to start from, checks of what an operation made).
- The app's logic lives in `cad/cad-ui` (model, view, messages), apart from the window, so most of it is tested without one.
- Benchmarks (`bench/`) are separate programs, not tests.

Add a test with a fix: say what was wrong in a way a test can fail.

## Commits and pull requests

- The history has short commit messages in the imperative mood, a sentence with a capital letter and no prefix ("Add fillets", "Fix nan bug"). Do the same; a line more of explanation after a blank line is welcome when the reason is not obvious.
- One change per pull request, kept small enough to read. Say what it does and how you checked it; the pull request template asks for that.
- Do not commit STEP, IGES or other CAD files, meshes, videos or other large binaries. Test data is built in code or fetched by the benchmark from a pinned address (`bench/boolean/datasets.tsv`). Pictures for the documentation go under `docs/images/` and stay below 400 kB.
- Do not commit generated files: `target/`, `*.shapecraft.views/`, `*.parts/` and `__pycache__/` are ignored for that reason.
- Parts files (`.shapecraft`) are not a stable format; a change that breaks old files needs no migration but should say so.

## Licence of contributions

ShapeCraft is licensed under MIT OR Apache-2.0. Unless you say otherwise, a contribution you submit is licensed the same way, without further terms. Only send code that you wrote or that you may license so, and name the origin of anything you copy.

## Reporting problems

Bugs and ideas go to the issue tracker; use the templates. Security problems go through [`SECURITY.md`](SECURITY.md), not a public issue.
