#!/bin/sh
# build.sh [out dir]: the application for the browser, as a static folder
# (index.html, workpiece.js, workpiece_bg.wasm, the icon, and the
# assistant: assistant/ with its scripts, the Python layer as
# workpiece-python.zip and Pyodide, vendored by vendor.sh); by default
# website/app/dist. Serve the folder over HTTP (a page from file:// cannot
# load WebAssembly modules).
#
# Needs: the Rust target (rustup target add wasm32-unknown-unknown) and
# wasm-bindgen of the version Cargo.lock has
# (cargo install wasm-bindgen-cli --version <it>; one trunk downloaded is
# found too). wasm-opt (binaryen) makes the file smaller, if there is one.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
OUT=${1:-$HERE/dist}
case $OUT in /*) ;; *) OUT=$(pwd)/$OUT ;; esac

# The wasm-bindgen command line must be of the same version as the crate.
VERSION=$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p;}' "$REPO/Cargo.lock")
BINDGEN=
for candidate in "${WASM_BINDGEN:-}" wasm-bindgen "$HOME/.cache/trunk/wasm-bindgen-$VERSION/wasm-bindgen"; do
  [ -n "$candidate" ] || continue
  if command -v "$candidate" >/dev/null 2>&1 &&
    "$candidate" --version 2>/dev/null | grep -q " $VERSION\$"; then
    BINDGEN=$candidate
    break
  fi
done
if [ -z "$BINDGEN" ]; then
  echo "build.sh: no wasm-bindgen $VERSION; install it: cargo install wasm-bindgen-cli --version $VERSION" >&2
  exit 1
fi
OPT=
for candidate in "${WASM_OPT:-}" wasm-opt "$HOME"/.cache/trunk/wasm-opt-*/bin/wasm-opt; do
  [ -n "$candidate" ] || continue
  if command -v "$candidate" >/dev/null 2>&1; then
    OPT=$candidate
    break
  fi
done

cd "$REPO"
cargo build --profile web --target wasm32-unknown-unknown -p workpiece-app
WASM=$REPO/target/wasm32-unknown-unknown/web/workpiece-app.wasm

rm -rf "$OUT"
mkdir -p "$OUT"
"$BINDGEN" --target web --no-typescript --out-dir "$OUT" --out-name workpiece "$WASM"
if [ -n "$OPT" ]; then
  # (The features are those Rust's wasm32 target uses by default.)
  "$OPT" -O3 --enable-bulk-memory --enable-nontrapping-float-to-int \
    --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
    "$OUT/workpiece_bg.wasm" -o "$OUT/workpiece_bg.wasm.opt"
  mv "$OUT/workpiece_bg.wasm.opt" "$OUT/workpiece_bg.wasm"
else
  echo "build.sh: no wasm-opt: the file is left as wasm-bindgen wrote it" >&2
fi
cp "$HERE/index.html" "$OUT/"
cp "$REPO/hosts/app/assets/workpiece.png" "$OUT/"

# The assistant, loaded once it is first opened: its scripts, the Python layer from this
# repository's sources (python/workpiece, with agent/README.md as the README it reads its
# guide from), and Pyodide.
mkdir -p "$OUT/assistant"
for f in "$HERE"/assistant/*.js; do cp "$f" "$OUT/assistant/"; done
python3 - "$REPO" "$OUT/assistant/workpiece-python.zip" <<'PY'
import os, sys, zipfile
repo, out = sys.argv[1:]
package = os.path.join(repo, "python", "workpiece")
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for name in sorted(os.listdir(package)):
        if name.endswith(".py"):
            z.write(os.path.join(package, name), f"workpiece/{name}")
    z.write(os.path.join(repo, "agent", "README.md"), "workpiece/README.md")
PY
"$HERE/vendor.sh" "$OUT"

size() { wc -c <"$1" | tr -d ' '; }
raw=$(size "$OUT/workpiece_bg.wasm")
gz=$(gzip -9 -c "$OUT/workpiece_bg.wasm" | wc -c | tr -d ' ')
line="workpiece_bg.wasm: $raw bytes, $gz gzipped"
if command -v brotli >/dev/null 2>&1; then
  br=$(brotli -c -q 11 "$OUT/workpiece_bg.wasm" | wc -c | tr -d ' ')
  line="$line, $br with brotli"
fi
echo "$line"
echo "the assistant (loaded when it is opened): $(du -sk "$OUT/assistant" | cut -f1) KB"
echo "written to $OUT"
