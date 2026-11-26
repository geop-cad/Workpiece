#!/bin/sh
# build.sh [out dir]: the application for the browser, as a static folder
# (index.html, the program as workpiece-<hash>.js and workpiece_bg-<hash>.wasm,
# the icon, and the assistant: assistant/ with its scripts, the Python layer as
# workpiece-python.zip and Pyodide, vendored by vendor.sh); by default
# website/app/dist. Serve the folder over HTTP (a page from file:// cannot
# load WebAssembly modules).
#
# Needs: the Rust target (rustup target add wasm32-unknown-unknown) and
# wasm-bindgen of the version Cargo.lock has
# (cargo install wasm-bindgen-cli --version <it>; one trunk downloaded is
# found too), and wasm-opt of binaryen's release BINARYEN below (the one
# .github/workflows/site.yml installs, so that a local build is the one
# deployed; WASM_OPT names it if it is not on the PATH).
#
# The program's two files are named by their contents (workpiece-<hash>.js,
# workpiece_bg-<hash>.wasm), so a browser keeps them for good (_headers);
# index.html, thread.js and the assistant's worker are written with those names.
#
# The program is built with atomics and a shared memory: the build engine's threads are Web
# Workers that run it on the page's memory (thread.js, hosts/app/src/web.rs). The standard
# library is built for that from its sources (rust-src of the toolchain: rustup component add
# rust-src), with RUSTC_BOOTSTRAP for -Z build-std on a stable toolchain. Threads need a page
# that is cross-origin isolated (the headers in website/_headers); without, it builds on the
# page's thread.
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
BINARYEN=131
OPT=
for candidate in "${WASM_OPT:-}" wasm-opt "$HOME/.cache/trunk/wasm-opt-version_$BINARYEN/bin/wasm-opt"; do
  [ -n "$candidate" ] || continue
  if command -v "$candidate" >/dev/null 2>&1 &&
    "$candidate" --version 2>/dev/null | grep -q "^wasm-opt version $BINARYEN "; then
    OPT=$candidate
    break
  fi
done
if [ -z "$OPT" ]; then
  echo "build.sh: no wasm-opt of binaryen $BINARYEN; get it from https://github.com/WebAssembly/binaryen/releases/tag/version_$BINARYEN" >&2
  exit 1
fi

cd "$REPO"
# (Up to 2 GiB of memory: a shared memory is declared with its largest size, which a phone's
# browser must be able to set aside.)
# (The linker is told to share the memory, take it from who starts the program, and export
# what wasm-bindgen sets each thread's own statics up with.)
LINK="--shared-memory --import-memory --max-memory=2147483648 --export=__wasm_init_tls --export=__tls_size --export=__tls_align --export=__tls_base"
RUSTC_BOOTSTRAP=1 \
  CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals $(for a in $LINK; do printf ' -C link-arg=%s' "$a"; done)" \
  cargo build --profile web --target wasm32-unknown-unknown -p workpiece-app -Z build-std=std,panic_abort
WASM=${CARGO_TARGET_DIR:-$REPO/target}/wasm32-unknown-unknown/web/workpiece-app.wasm

rm -rf "$OUT"
mkdir -p "$OUT"
"$BINDGEN" --target web --no-typescript --out-dir "$OUT" --out-name workpiece "$WASM"
# -O3, not -Oz: -Oz saves 2 % of the file but nothing once it is compressed, as served.
# Without -g it drops the names; the producers section goes too. (The features are
# those Rust's wasm32 target uses by default.)
"$OPT" -O3 --strip-producers --enable-bulk-memory --enable-nontrapping-float-to-int \
  --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue \
  --enable-threads \
  "$OUT/workpiece_bg.wasm" -o "$OUT/workpiece_bg.wasm.opt"
mv "$OUT/workpiece_bg.wasm.opt" "$OUT/workpiece_bg.wasm"
cp "$REPO/hosts/app/assets/workpiece.png" "$OUT/"
# The emoji fonts, which the program fetches once it is up (hosts/app/src/web.rs) rather than
# have them in its download: those of the egui it is built with.
mkdir -p "$OUT/fonts"
cargo metadata --format-version 1 --locked --filter-platform wasm32-unknown-unknown |
  python3 -c '
import json, os, shutil, sys
out = sys.argv[1]
fonts = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "epaint_default_fonts"]
if len(fonts) != 1:
    sys.exit("build.sh: not one epaint_default_fonts among the dependencies")
folder = os.path.join(os.path.dirname(fonts[0]["manifest_path"]), "fonts")
for name in ("NotoEmoji-Regular.ttf", "emoji-icon-font.ttf"):
    shutil.copy(os.path.join(folder, name), os.path.join(out, name))
' "$OUT/fonts"

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

# The names by contents: the wasm's first, which its loader names, then the loader's, which
# the page, the build threads' script and the assistant's worker name. The page is given the
# wasm's size too (for its progress bar: what is served is compressed, so the download's own
# length is not it).
python3 - "$OUT" "$HERE/index.html" <<'PY'
import hashlib, os, sys
out, page = sys.argv[1:]
def path(name):
    return os.path.join(out, name)
def replace(text, old, new, where):
    if text.count(old) != 1:
        sys.exit(f"build.sh: {where} does not name {old!r} once")
    return text.replace(old, new)
def named(name):
    with open(path(name), "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()[:16]
    stem, ext = os.path.splitext(name)
    os.rename(path(name), path(f"{stem}-{digest}{ext}"))
    return f"{stem}-{digest}{ext}"
wasm = named("workpiece_bg.wasm")
with open(path("workpiece.js")) as f:
    js = f.read()
js = replace(js, "new URL('workpiece_bg.wasm', import.meta.url)", f"new URL('{wasm}', import.meta.url)", "workpiece.js")
with open(path("workpiece.js"), "w") as f:
    f.write(js)
js = named("workpiece.js")
with open(page) as f:
    html = f.read()
html = html.replace("%WORKPIECE_JS%", js).replace("%WORKPIECE_WASM%", wasm)
html = replace(html, "%WORKPIECE_WASM_BYTES%", str(os.path.getsize(path(wasm))), "index.html")
if "%WORKPIECE" in html:
    sys.exit("build.sh: index.html has a name build.sh does not fill in")
with open(path("index.html"), "w") as f:
    f.write(html)
with open(os.path.join(os.path.dirname(page), "thread.js")) as f:
    text = f.read()
with open(path("thread.js"), "w") as f:
    f.write(replace(text, '"./workpiece.js"', f'"./{js}"', "thread.js"))
worker = path("assistant/worker.js")
with open(worker) as f:
    text = f.read()
with open(worker, "w") as f:
    f.write(replace(text, '"../workpiece.js"', f'"../{js}"', "assistant/worker.js"))
PY

size() { wc -c <"$1" | tr -d ' '; }
WASM_OUT=$(ls "$OUT"/workpiece_bg-*.wasm)
raw=$(size "$WASM_OUT")
gz=$(gzip -9 -c "$WASM_OUT" | wc -c | tr -d ' ')
line="$(basename "$WASM_OUT"): $raw bytes, $gz gzipped"
if command -v brotli >/dev/null 2>&1; then
  br=$(brotli -c -q 11 "$WASM_OUT" | wc -c | tr -d ' ')
  line="$line, $br with brotli"
fi
echo "$line"
echo "the assistant (loaded when it is opened): $(du -sk "$OUT/assistant" | cut -f1) KB"
echo "written to $OUT"
