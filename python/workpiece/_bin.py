"""Where the compiled programs and the README are, and the version.

A wheel carries `workpiece` (the command line) and `workpiece-app` (the desktop application)
in `workpiece/bin/`. A source checkout has them in `target/release` (or `target/debug`) after
`cargo build --release -p workpiece-cli -p workpiece-app`. `WORKPIECE_BIN` and
`WORKPIECE_APP_BIN` name other copies.
"""

from __future__ import annotations

import os
import re
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
_CHECKOUT = os.path.normpath(os.path.join(_HERE, "..", ".."))
_EXE = ".exe" if sys.platform == "win32" else ""


def _version() -> str:
    try:
        from importlib.metadata import version
        return version("workpiece")
    except Exception:
        pass
    # A source checkout that is not installed: the workspace's own version.
    try:
        with open(os.path.join(_CHECKOUT, "Cargo.toml")) as f:
            m = re.search(r'\[workspace\.package\][^\[]*?\nversion\s*=\s*"([^"]+)"', f.read())
        if m:
            return m[1]
    except OSError:
        pass
    return "0+unknown"


__version__ = _version()


def _find(program: str, env: str, hint: str) -> str:
    candidates = [
        os.environ.get(env),
        os.path.join(_HERE, "bin", program + _EXE),
        os.path.join(_CHECKOUT, "target", "release", program + _EXE),
        os.path.join(_CHECKOUT, "target", "debug", program + _EXE),
    ]
    for path in candidates:
        if path and os.path.isfile(path):
            return path
    raise RuntimeError(
        f"the {program} program was not found. A pip install of workpiece brings it; "
        f"in a source checkout build it first: {hint}")


def command_line() -> str:
    """The path of the `workpiece` command line program."""
    return _find("workpiece", "WORKPIECE_BIN", "cargo build --release -p workpiece-cli")


def app() -> str:
    """The path of the `workpiece-app` desktop application."""
    return _find("workpiece-app", "WORKPIECE_APP_BIN", "cargo build --release -p workpiece-app")


def readme() -> str:
    """The text of the layer's README (the wheel has a copy; a checkout has agent/README.md)."""
    for path in (os.path.join(_HERE, "README.md"), os.path.join(_CHECKOUT, "agent", "README.md")):
        if os.path.isfile(path):
            with open(path, encoding="utf-8") as f:
                return f.read()
    raise RuntimeError("README.md was not found beside the package")


def run(path: str, args: list) -> int:
    """Runs a program with `args` in place of this process (so signals and the exit code are its own)."""
    if sys.platform == "win32":
        import subprocess
        return subprocess.call([path, *args])
    sys.stdout.flush()
    os.execv(path, [path, *args])
