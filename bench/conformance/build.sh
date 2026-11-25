#!/bin/sh
# Builds the workpiece adapter (our own code: outside the sandbox) into target/conformance.
#   build.sh            the working tree
#   build.sh --head     the last commit, exported to $WORKPIECE_BENCH_DIR/workpiece-head-snapshot
#                       (for when the working tree does not compile); the adapter crate is
#                       linked in from the working tree
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
BENCH=${WORKPIECE_BENCH_DIR:-$HOME/.cache/workpiece-bench}
SRC=$REPO
if [ "${1:-}" = "--head" ]; then
  SRC=$BENCH/workpiece-head-snapshot
  rm -rf "$SRC"
  mkdir -p "$SRC"
  git -C "$REPO" archive HEAD | tar -x -C "$SRC"
  rm -rf "$SRC/bench/conformance"
  ln -s "$REPO/bench/conformance" "$SRC/bench/conformance"
  grep -q '"bench/conformance"' "$SRC/Cargo.toml" ||
    sed -i 's|    "bench/boolean",|    "bench/boolean",\n    "bench/conformance",|' "$SRC/Cargo.toml"
  mkdir -p "$BENCH/conformance-sandbox/out"
  git -C "$REPO" rev-parse HEAD > "$BENCH/conformance-sandbox/out/workpiece-commit.txt"
fi
cd "$SRC"
CARGO_TARGET_DIR=$REPO/target/conformance cargo build --release -p workpiece-conformance-adapter
