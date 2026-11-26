#!/bin/sh
# assemble.sh <out dir>: the site as it is published: the pages build_site.py writes (home page,
# the examples' scripts, privacy page, 404, robots.txt, sitemap.xml) with style.css, assets/,
# analytics.js and Cloudflare Pages' _headers, the book (built from docs/book) as docs/, the
# examples as examples/ and, once app/build.sh has built it, the application for the browser as
# app/. SITE_URL (default: site.json's) is where it is served from.
# Needs mdbook on the PATH. MDBOOK_OUTPUT__HTML__SITE_URL sets where the book is served from.
# WORKPIECE_APP names the folder app/build.sh wrote, if not app/dist.
# Statistics: with POSTHOG_BUNDLE (PostHog's array.no-external.js) and POSTHOG_KEY (the project's
# public key) both set, the bundle is served from vendor/posthog/ and the key is written into the
# site's analytics.js; otherwise the site sends nothing.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/.." && pwd)
OUT=${1:?usage: assemble.sh <out dir>}
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$HERE/style.css" "$HERE/analytics.js" "$HERE/_headers" "$OUT/"
cp -r "$HERE/assets" "$OUT/assets"
mdbook build "$REPO/docs/book" -d "$OUT/docs"
# The examples: their index, part files, scripts, pictures and drawings (not what
# the scripts keep beside the part files).
if [ -d "$REPO/examples" ]; then
  (cd "$REPO/examples" && find . -name '*.views' -prune -o -type f \
    \( -name '*.wpc' -o -name '*.json' -o -name '*.png' -o -name '*.pdf' -o -name '*.svg' \
    -o -name '*.py' -o -name '*.step' -o -name '*.stp' \) -print) | while read -r f; do
    mkdir -p "$OUT/examples/$(dirname "$f")"
    cp "$REPO/examples/$f" "$OUT/examples/$f"
  done
fi
# The application, as app/build.sh wrote it.
APP=${WORKPIECE_APP:-$HERE/app/dist}
if [ -f "$APP/index.html" ]; then
  cp -r "$APP" "$OUT/app"
else
  echo "assemble.sh: no application in $APP (run website/app/build.sh): the site has none" >&2
fi
# The statistics' bundle and key (see analytics.js).
key=$(printf '%s' "${POSTHOG_KEY:-}" | tr -d '[:space:]')
if [ -n "$key" ] && [ -f "${POSTHOG_BUNDLE:-}" ]; then
  case $key in
    *[!A-Za-z0-9_-]*) echo "assemble.sh: POSTHOG_KEY has characters a PostHog key does not have" >&2; exit 1 ;;
  esac
  mkdir -p "$OUT/vendor/posthog"
  cp "$POSTHOG_BUNDLE" "$OUT/vendor/posthog/array.no-external.js"
  sed -i "s/__POSTHOG_KEY__/$key/" "$OUT/analytics.js"
  grep -q "$key" "$OUT/analytics.js"
  echo "assemble.sh: statistics on"
else
  echo "assemble.sh: no POSTHOG_KEY and POSTHOG_BUNDLE: the site sends no statistics"
fi
# The pages (they read the examples, the benchmark results and the application's size).
python3 "$HERE/build_site.py" "$OUT"
