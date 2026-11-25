"""`python -m workpiece.cli ...` and the console script `workpiece`: the command line program.

Everything is passed on: `workpiece make recipe.json -o part.wpc`, `workpiece report
part.wpc`, `workpiece kinds`, `workpiece --version`.
"""

from __future__ import annotations

import sys

from . import _bin


def main(argv=None) -> int:
    try:
        return _bin.run(_bin.command_line(), list(sys.argv[1:] if argv is None else argv))
    except RuntimeError as e:
        print(f"workpiece: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
