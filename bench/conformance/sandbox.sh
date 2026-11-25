#!/bin/sh
# Runs a command with no network, /home replaced by a tmpfs and only the sandbox folder
# ($WORKPIECE_BENCH_DIR/conformance-sandbox, seen as /work) writable. The fetched crates
# (/work/cargo-home) and the toolchain are read-only. This script lives outside /work on
# purpose: nothing run inside the sandbox can change it.
#
#   WORKPIECE_BENCH_DIR   where the clones and the sandbox live (default ~/.cache/workpiece-bench)
#   sandbox.sh [--ro HOST_PATH SANDBOX_PATH]... -- command args...
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
BENCH=${WORKPIECE_BENCH_DIR:-$HOME/.cache/workpiece-bench}
SB=$BENCH/conformance-sandbox
[ -d "$SB/cargo-home" ] || { echo "no sandbox at $SB: set it up as bench/conformance/README.md says (WORKPIECE_BENCH_DIR names its parent folder)" >&2; exit 2; }
TC=$HOME/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu
EXTRA=""
while [ "${1:-}" = "--ro" ]; do EXTRA="$EXTRA --ro-bind $2 $3"; shift 3; done
[ "${1:-}" = "--" ] && shift
# shellcheck disable=SC2086
exec bwrap --unshare-all --die-with-parent \
  --ro-bind /usr /usr --symlink usr/lib /lib --symlink usr/lib64 /lib64 --symlink usr/bin /bin \
  --ro-bind /etc/alternatives /etc/alternatives --ro-bind /etc/ld.so.cache /etc/ld.so.cache \
  --proc /proc --dev /dev --tmpfs /tmp --tmpfs /home --tmpfs /run \
  --ro-bind "$TC" /toolchain \
  --bind "$SB" /work --ro-bind "$SB/cargo-home" /work/cargo-home $EXTRA \
  --clearenv --setenv PATH /toolchain/bin:/usr/bin --setenv HOME /tmp \
  --setenv CARGO_HOME /work/cargo-home --setenv CARGO_NET_OFFLINE true --setenv LANG C.UTF-8 \
  --chdir /work "$@"
