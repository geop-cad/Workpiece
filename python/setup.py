"""Builds the wheel: the Python package plus the two programs, built with cargo.

The wheel is platform specific but not Python specific (`py3-none-<platform>`): the programs are
plain executables in `shapecraft/bin/`. Where they come from:

- `SHAPECRAFT_PREBUILT=<dir>`: copy `shapecraft` and `shapecraft-app` from there (CI builds them
  first, for the target it wants, and for macOS joins two architectures into one).
- otherwise `cargo build --release -p shapecraft-cli -p shapecraft-app` in the workspace: the
  repository's root when this folder is `python/` of a checkout, or `rust/` of an sdist.

`SHAPECRAFT_WHEEL_PLAT` sets the platform tag (`manylinux_2_28_x86_64`, `macosx_11_0_arm64`,
`macosx_10_13_universal2`, `win_amd64`); without it the building machine's own is used.
The version is the workspace's `version` in Cargo.toml, read here and nowhere else.
"""

import os
import re
import shutil
import subprocess
import sys

from setuptools import setup
from setuptools.command.build_py import build_py
from setuptools.command.sdist import sdist

try:
    from setuptools.command.bdist_wheel import bdist_wheel
except ImportError:  # setuptools older than 70.1
    from wheel.bdist_wheel import bdist_wheel

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = ".exe" if sys.platform == "win32" else ""
PROGRAMS = ["shapecraft", "shapecraft-app"]
# What an sdist carries of the repository so that cargo can build it there.
WORKSPACE = ["Cargo.toml", "Cargo.lock", "LICENSE-MIT", "LICENSE-APACHE", "README.md",
             "core", "lib", "ops", "cad", "hosts", "bench"]


def workspace() -> str:
    for path in (os.path.join(HERE, ".."), os.path.join(HERE, "rust")):
        if os.path.isfile(os.path.join(path, "Cargo.toml")) and os.path.isdir(os.path.join(path, "hosts")):
            return os.path.normpath(path)
    raise SystemExit("the Rust workspace was not found beside python/ or in rust/")


def version() -> str:
    with open(os.path.join(workspace(), "Cargo.toml")) as f:
        m = re.search(r'\[workspace\.package\][^\[]*?\nversion\s*=\s*"([^"]+)"', f.read())
    if not m:
        raise SystemExit("no [workspace.package] version in Cargo.toml")
    return m[1]


def readme() -> str:
    for path in (os.path.join(workspace(), "agent", "README.md"), os.path.join(HERE, "rust", "agent", "README.md")):
        if os.path.isfile(path):
            return path
    raise SystemExit("agent/README.md was not found")


class BuildPy(build_py):
    def run(self):
        super().run()
        bin_dir = os.path.join(self.build_lib, "shapecraft", "bin")
        os.makedirs(bin_dir, exist_ok=True)
        shutil.copy(readme(), os.path.join(self.build_lib, "shapecraft", "README.md"))
        prebuilt = os.environ.get("SHAPECRAFT_PREBUILT")
        if prebuilt is None:
            root = workspace()
            print("building the programs with cargo (release)", flush=True)
            try:
                subprocess.check_call(
                    ["cargo", "build", "--release", "-p", "shapecraft-cli", "-p", "shapecraft-app"], cwd=root)
            except FileNotFoundError:
                raise SystemExit("cargo was not found: install Rust from https://rustup.rs to build from source")
            prebuilt = os.environ.get("CARGO_TARGET_DIR", os.path.join(root, "target")) + os.sep + "release"
        for name in PROGRAMS:
            shutil.copy(os.path.join(prebuilt, name + EXE), os.path.join(bin_dir, name + EXE))


class Wheel(bdist_wheel):
    def finalize_options(self):
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self):
        _, _, plat = super().get_tag()
        return "py3", "none", os.environ.get("SHAPECRAFT_WHEEL_PLAT", plat)


class Sdist(sdist):
    def make_release_tree(self, base_dir, files):
        super().make_release_tree(base_dir, files)
        root, dest = workspace(), os.path.join(base_dir, "rust")
        os.makedirs(dest, exist_ok=True)
        skip = shutil.ignore_patterns("target", ".git", "__pycache__", "*.shapecraft", "*.parts")
        for name in WORKSPACE:
            src = os.path.join(root, name)
            if os.path.isdir(src):
                shutil.copytree(src, os.path.join(dest, name), ignore=skip)
            else:
                shutil.copy(src, dest)
        os.makedirs(os.path.join(dest, "agent"), exist_ok=True)
        shutil.copy(readme(), os.path.join(dest, "agent", "README.md"))


setup(version=version(), cmdclass={"build_py": BuildPy, "bdist_wheel": Wheel, "sdist": Sdist})
