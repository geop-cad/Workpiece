#!/usr/bin/env python3
"""Writes the THIRD-PARTY notice: every crate the two programs are built from, with its
version, licence and repository, and the licences of the fonts the app carries.

    python3 scripts/third_party.py THIRD-PARTY.txt [--target x86_64-pc-windows-msvc]

Without --target the dependencies for every platform are listed (a superset of what one
platform's build contains). Development dependencies (tests, benchmarks) are left out: the
walk starts at `shapecraft-app` and `shapecraft-cli` and follows normal and build dependencies.
"""

import argparse
import glob
import json
import os
import subprocess
import sys

ROOTS = ["shapecraft-app", "shapecraft-cli"]
FONT_LICENCES = ["OFL.txt", "UFL.txt", "Hack-Regular.txt", "emoji-icon-font-mit-license.txt"]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--target")
    a = ap.parse_args()
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked"]
    if a.target:
        cmd += ["--filter-platform", a.target]
    meta = json.loads(subprocess.run(cmd, cwd=root, check=True, capture_output=True, text=True).stdout)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    todo = [i for i, p in packages.items() if p["name"] in ROOTS]
    seen = set(todo)
    while todo:
        for dep in nodes[todo.pop()]["deps"]:
            if any(k["kind"] != "dev" for k in dep["dep_kinds"]) and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                todo.append(dep["pkg"])
    ours = {p["id"] for p in meta["packages"] if p["source"] is None}
    found = sorted((packages[i] for i in seen - ours), key=lambda p: (p["name"], p["version"]))
    lines = [
        "ShapeCraft is licensed under MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE).",
        "",
        "It is built from the following third-party crates, each under the licence named",
        "(an SPDX expression; where it offers a choice, either may be used).",
        "",
        f"{'crate':36} {'version':14} licence",
        f"{'-' * 36} {'-' * 14} {'-' * 30}",
    ]
    for p in found:
        lines.append(f"{p['name']:36} {p['version']:14} {p['license'] or 'see ' + (p['repository'] or 'the crate')}")
    lines += ["", f"{len(found)} crates.", ""]
    fonts = [d for p in found if p["name"] == "epaint_default_fonts" for d in [os.path.join(os.path.dirname(p["manifest_path"]), "fonts")]]
    if fonts:
        lines += ["=" * 78, "Fonts carried by the app (from the crate epaint_default_fonts)", "=" * 78, ""]
        for name in FONT_LICENCES:
            for path in glob.glob(os.path.join(fonts[0], name)):
                lines += [f"--- {name} ---", open(path, encoding="utf-8", errors="replace").read().rstrip(), ""]
    with open(a.out, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"{a.out}: {len(found)} crates", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
