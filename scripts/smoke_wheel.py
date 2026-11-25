#!/usr/bin/env python3
"""Installs a wheel into a fresh virtual environment and builds a part with it.

    python3 scripts/smoke_wheel.py dist/workpiece-0.1.0-....whl

Checks that `python -m workpiece --version`, the `workpiece` console script and the layer
find the programs inside the wheel (nothing on the PATH, nothing from this checkout), and that
the flange example builds with the right volume. The example runs in a temporary folder.
"""

import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main() -> int:
    wheel = os.path.abspath(sys.argv[1])
    env = {k: v for k, v in os.environ.items() if not k.startswith("WORKPIECE_") and k != "PYTHONPATH"}
    with tempfile.TemporaryDirectory() as tmp:
        venv = os.path.join(tmp, "venv")
        subprocess.check_call([sys.executable, "-m", "venv", venv])
        scripts = os.path.join(venv, "Scripts" if sys.platform == "win32" else "bin")
        python = os.path.join(scripts, "python" + (".exe" if sys.platform == "win32" else ""))
        subprocess.check_call([python, "-m", "pip", "install", "--quiet", wheel], cwd=tmp, env=env)

        def out(*cmd: str, cwd: str = tmp) -> str:
            done = subprocess.run(list(cmd), cwd=cwd, env=env, capture_output=True, text=True)
            print("+", " ".join(cmd), "->", (done.stdout + done.stderr).strip().splitlines()[-1:] or "")
            if done.returncode != 0:
                raise SystemExit(f"failed ({done.returncode}):\n{done.stdout}\n{done.stderr}")
            return done.stdout

        out(python, "-m", "workpiece", "--help")
        assert re.match(r"Workpiece \d+\.\d+\.\d+", out(python, "-m", "workpiece", "--version"))
        assert re.match(r"workpiece \d+\.\d+\.\d+", out(os.path.join(scripts, "workpiece"), "--version"))
        out(python, "-m", "workpiece.cli", "kinds")
        out(python, "-m", "workpiece.tools", "help")
        example = os.path.join(ROOT, "agent", "examples", "flange.py")
        with open(example) as f:
            text = "".join(line for line in f if "sys.path.insert" not in line)
        work = os.path.join(tmp, "work")
        os.makedirs(work)
        with open(os.path.join(work, "flange.py"), "w") as f:
            f.write(text)
        said = out(python, "flange.py", "flange.workpiece", cwd=work)
        volume = float(re.search(r"^volume ([\d.]+)", said, re.M)[1])
        assert abs(volume - 80917.77) < 0.5, volume
        shown = out(python, "-c", "import workpiece; print(workpiece.__file__)", cwd=work).strip()
        assert os.path.realpath(ROOT) not in os.path.realpath(shown), shown
    print("wheel ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
