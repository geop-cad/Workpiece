#!/bin/sh
# Runs brep-conformance on an adapter, inside the sandbox (see sandbox.sh).
#   run.sh workpiece     [suite options, e.g. --family fillets/ --timeout 60]
#   run.sh springrep [suite options]
# Results: $WORKPIECE_BENCH_DIR/conformance-sandbox/out/$OUT.{json,html,log} (default folder
# ~/.cache/workpiece-bench; OUT defaults to the kernel's name). THREADS sets the number of
# workers (12). SUITE and SPRINGREP_ADAPTER name the sandbox's folders that hold the suite and
# springrep's adapter (default: suite, springrep-adapter).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
NAME=$1
shift
case $NAME in
  workpiece) ADAPTER=/workpiece-bin/workpiece-adapter ;;
  springrep) ADAPTER=/work/${SPRINGREP_ADAPTER:-springrep-adapter}/target/release/springrep-adapter ;;
  *) echo "unknown kernel $NAME" >&2; exit 2 ;;
esac
exec "$HERE/sandbox.sh" --ro "$REPO/target/conformance/release" /workpiece-bin -- sh -c '
  adapter=$0; out=$1; threads=$2; suite=$3; shift 3
  cd /work/out && exec "/work/$suite/target/release/brep-conformance" --adapter "$adapter" \
    --threads "$threads" --out "$out.json" --html "$out.html" "$@" > "$out.log" 2>&1' \
  "$ADAPTER" "${OUT:-$NAME}" "${THREADS:-12}" "${SUITE:-suite}" "$@"
