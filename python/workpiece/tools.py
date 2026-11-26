"""The helper commands beside the layer: `python -m workpiece.tools help|sheet|crop|tile`
(or the console script `workpiece-tools`).

    python -m workpiece.tools help [topic]     one section of the README (no topic: the topics)
    python -m workpiece.tools sheet part.wpc
    python -m workpiece.tools crop sheet.pdf X Y W H -o out.png
    python -m workpiece.tools tile picture.png --grid 3x2 -o tiles
"""

from __future__ import annotations

import json
import os
import re
import sys
import tempfile

from . import _backend, _bin, Report, crop, tile

def _slug(title: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")


def _sections(text: str) -> list:
    """(level, title, slug, first line, end line) of the README's `##` / `###` headings."""
    lines, found, fence = text.splitlines(), [], False
    for n, line in enumerate(lines):
        if line.startswith("```"):
            fence = not fence
        m = None if fence else re.match(r"(##|###) (.*)", line)
        if m:
            found.append([len(m[1]), m[2].strip(), _slug(m[2]), n, len(lines)])
    for k, a in enumerate(found):
        for b in found[k + 1:]:
            if b[0] <= a[0]:
                a[4] = b[3]
                break
    return [tuple(x) for x in found]


def help_topic(topic: str | None = None) -> str:
    """The README's section `topic` (a heading, by its slug or a part of it: `drawing`,
    `centre-lines`, `assemblies`, `quick`, `check`), or the list of topics."""
    text = _bin.readme()
    lines, sections = text.splitlines(), _sections(text)
    if not topic:
        return "topics (python -m workpiece.tools help <topic>):\n" + "\n".join(
            f"{'  ' * (lv - 2)}{slug}  - {title}" for lv, title, slug, _, _ in sections)
    want = _slug(topic)
    hit = next((s for s in sections if s[2] == want), None) or next((s for s in sections if s[2].startswith(want)), None) \
        or next((s for s in sections if want in s[2]), None)
    if not hit:
        raise SystemExit(f"no topic {topic!r}\n" + help_topic())
    return "\n".join(lines[hit[3]:hit[4]]).rstrip()


def main(argv: list) -> int:
    import argparse
    ap = argparse.ArgumentParser(prog="python -m workpiece.tools", description="Helpers beside the layer.")
    sub = ap.add_subparsers(dest="command", required=True)
    h = sub.add_parser("help", help="print one section of README.md (no topic: list the topics)")
    h.add_argument("topic", nargs="?")
    c = sub.add_parser("crop", help="cut a box out of a sheet PDF (mm from its lower left) or an image (pixels)")
    c.add_argument("path")
    c.add_argument("x", type=float); c.add_argument("y", type=float); c.add_argument("w", type=float); c.add_argument("h", type=float)
    c.add_argument("-o", "--out", required=True)
    c.add_argument("-m", "--margin", type=float, default=0.0, help="widen the box on every side")
    c.add_argument("--long-side", type=int, default=1200, help="pixels on the long side")
    t = sub.add_parser("tile", help="cut an image into overlapping tiles at 2x")
    t.add_argument("path")
    t.add_argument("--grid", default="3x2", help="columns x rows")
    t.add_argument("-o", "--out", default="tiles")
    t.add_argument("--scale", type=float, default=2.0)
    s = sub.add_parser("sheet", help="print the sheet inventory of a built part's drawings")
    s.add_argument("part")
    a = ap.parse_args(argv)
    if a.command == "help":
        print(help_topic(a.topic))
    elif a.command == "crop":
        print(crop(a.path, (a.x, a.y, a.w, a.h), a.out, a.long_side, a.margin))
    elif a.command == "tile":
        cols, rows = (int(v) for v in a.grid.lower().split("x"))
        for file, box in tile(a.path, (cols, rows), a.out, a.scale):
            print(file, box)
    elif a.command == "sheet":
        with tempfile.TemporaryDirectory() as tmp:
            report = os.path.join(tmp, "r.json")
            done = _backend.run(["report", a.part, "--report", report])
            if done.returncode != 0:
                raise SystemExit(done.stderr.strip() or done.stdout.strip())
            with open(report) as f:
                print(Report(json.load(f)).sheet(show=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
