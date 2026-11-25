"""`python -m shapecraft.cli ...` and the console script `shapecraft`: the command line program.

Everything is passed on: `shapecraft make recipe.json -o part.shapecraft`, `shapecraft report
part.shapecraft`, `shapecraft kinds`, `shapecraft --version`.
"""

from __future__ import annotations

import sys

from . import _bin


def main(argv=None) -> int:
    try:
        return _bin.run(_bin.command_line(), list(sys.argv[1:] if argv is None else argv))
    except RuntimeError as e:
        print(f"shapecraft: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
