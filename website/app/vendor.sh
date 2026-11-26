#!/bin/sh
# vendor.sh <app dir>: what the assistant needs from elsewhere, copied into the built app
# (<app dir>/assistant/pyodide/), so that the page loads nothing from a CDN: Pyodide, the
# Python the assistant's scripts run in (MPL-2.0, https://pyodide.org). The release is pinned
# and its npm tarball checked against its sha512 before anything of it is used; the tarball
# is kept in website/app/.vendor/ so a second build needs no network. build.sh calls this.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${1:?usage: vendor.sh <app dir>}
VERSION=314.0.7
SHA512=0YvXxEhfEdpLfb/XkM2BFAeMROq0iMUX2bzzH9pOttyMcWkwq+HbE5uyuGD82LN7y2q+SNvi/6V5JEsOlD2R1A==
CACHE=$HERE/.vendor
TARBALL=$CACHE/pyodide-$VERSION.tgz
mkdir -p "$CACHE"
sum() { openssl dgst -sha512 -binary "$1" | base64 | tr -d '\n'; }
if [ ! -f "$TARBALL" ] || [ "$(sum "$TARBALL")" != "$SHA512" ]; then
  curl -sSfL --retry 3 -o "$TARBALL.part" "https://registry.npmjs.org/pyodide/-/pyodide-$VERSION.tgz"
  if [ "$(sum "$TARBALL.part")" != "$SHA512" ]; then
    rm -f "$TARBALL.part"
    echo "vendor.sh: pyodide-$VERSION.tgz is not the file it should be (sha512 differs)" >&2
    exit 1
  fi
  mv "$TARBALL.part" "$TARBALL"
fi
DEST=$OUT/assistant/pyodide
rm -rf "$DEST"
mkdir -p "$DEST"
# (Only what loading takes: the loader, the interpreter, its standard library, the lock file.)
tar -xzf "$TARBALL" -C "$DEST" --strip-components=1 \
  package/pyodide.mjs package/pyodide.asm.mjs package/pyodide.asm.wasm \
  package/python_stdlib.zip package/pyodide-lock.json package/package.json package/README.md
echo "vendor.sh: Pyodide $VERSION in $DEST"
