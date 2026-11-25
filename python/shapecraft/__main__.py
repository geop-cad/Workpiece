"""`python -m shapecraft [part.shapecraft]` and the console script `shapecraft-app`: the desktop app."""

from __future__ import annotations

import sys

from . import _bin

USAGE = """\
usage: python -m shapecraft [PART.shapecraft] [--look X,Y,Z,DISTANCE] [--from X,Y,Z] [--perspective]

Starts the ShapeCraft desktop application; a part file named is opened.

  python -m shapecraft --help           this text
  python -m shapecraft --version        the version
  python -m shapecraft.cli ...          the command line (also the console script `shapecraft`)
  python -m shapecraft.tools ...        helpers: help, sheet, crop, tile (also `shapecraft-tools`)

The authoring layer is `import shapecraft`; `python -m shapecraft.tools help` prints its README.
"""


def main(argv=None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if args[:1] in (["-h"], ["--help"]):
        print(USAGE, end="")
        return 0
    if args[:1] in (["-V"], ["--version"]):
        print(f"ShapeCraft {_bin.__version__}")
        return 0
    try:
        return _bin.run(_bin.app(), args)
    except RuntimeError as e:
        print(f"shapecraft: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
