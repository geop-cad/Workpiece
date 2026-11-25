"""`python -m workpiece [part.workpiece]` and the console script `workpiece-app`: the desktop app."""

from __future__ import annotations

import sys

from . import _bin

USAGE = """\
usage: python -m workpiece [PART.workpiece] [--look X,Y,Z,DISTANCE] [--from X,Y,Z] [--perspective]

Starts the Workpiece desktop application; a part file named is opened.

  python -m workpiece --help           this text
  python -m workpiece --version        the version
  python -m workpiece.cli ...          the command line (also the console script `workpiece`)
  python -m workpiece.tools ...        helpers: help, sheet, crop, tile (also `workpiece-tools`)

The authoring layer is `import workpiece`; `python -m workpiece.tools help` prints its README.
"""


def main(argv=None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if args[:1] in (["-h"], ["--help"]):
        print(USAGE, end="")
        return 0
    if args[:1] in (["-V"], ["--version"]):
        print(f"Workpiece {_bin.__version__}")
        return 0
    try:
        return _bin.run(_bin.app(), args)
    except RuntimeError as e:
        print(f"workpiece: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
