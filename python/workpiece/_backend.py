"""The one place the layer runs the `workpiece` command line.

`run(["make", "recipe.json", "-o", "part.wpc", ...])` runs a command and returns what it did:
`returncode`, `stdout`, `stderr` (as `subprocess.CompletedProcess`). On a desktop that is the
compiled program (`_bin.command_line()`) in a subprocess. In a browser (Pyodide, where there are
no subprocesses) it is the same command run by the WebAssembly kernel of the page: the page puts
the function `workpieceCommand` on the worker the script runs in, which runs the command on the
files it is handed and hands back the files it wrote. Both run the same code (the crate
`workpiece-make`), so a script builds the same part and reads the same report either way.
"""

from __future__ import annotations

import os
import subprocess
import sys

IN_BROWSER = sys.platform == "emscripten"

# The arguments that name a file or a folder (besides a command's first argument).
_PATH_FLAGS = ("-o", "--report", "--render", "--picture", "--in")


def run(args: list) -> "subprocess.CompletedProcess":
    """Runs the command line with `args` (without the program's name)."""
    if not IN_BROWSER:
        from . import _bin
        return subprocess.run([_bin.command_line(), *args], capture_output=True, text=True)
    return _in_browser([str(a) for a in args])


def _in_browser(args: list) -> "subprocess.CompletedProcess":
    import js  # noqa: F401  (Pyodide's bridge to the worker's JavaScript)
    from pyodide.ffi import to_js

    # Paths as the kernel sees them: absolute, the same as here.
    args = list(args)
    folders = set()
    for i, a in enumerate(args):
        if (i == 1 and args[0] in ("make", "report")) or (i > 0 and args[i - 1] in _PATH_FLAGS):
            args[i] = os.path.abspath(a)
    # What the command reads: the file it is of, and the folder the part is in (the parts it
    # places, its materials).
    given = {}
    if args[0] in ("make", "report") and len(args) > 1:
        given[args[1]] = None
        out = args[args.index("-o") + 1] if "-o" in args[:-1] else args[1]
        folders.add(os.path.dirname(out))
    if "--in" in args[:-1]:
        folders.add(args[args.index("--in") + 1])
    for folder in folders:
        if os.path.isdir(folder):
            for name in os.listdir(folder):
                path = os.path.join(folder, name)
                if os.path.isfile(path) and name.lower().endswith((".wpc", ".json", ".step", ".stp", ".igs", ".iges", ".stl", ".3mf")):
                    given[path] = None
    files = {}
    for path in given:
        if os.path.isfile(path):
            with open(path, "rb") as f:
                files[path] = f.read()
    done = js.workpieceCommand(to_js(args), to_js(files, dict_converter=js.Object.fromEntries))
    written = done.written.to_py()
    for path, data in written.items():
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        with open(path, "wb") as f:
            f.write(bytes(data))
    return subprocess.CompletedProcess(args, int(done.code), str(done.stdout), str(done.stderr))
