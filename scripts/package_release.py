#!/usr/bin/env python3
"""Packs a release archive from built programs.

    python3 scripts/package_release.py PLATFORM --bin DIR --version 0.1.0 --third-party THIRD-PARTY.txt --out DIR

PLATFORM is linux-x86_64 (.tar.gz), windows-x86_64 (.zip), macos-arm64 or macos-x86_64 (.dmg
holding Workpiece.app; the macOS ones need a Mac: they use sips, iconutil, codesign, hdiutil).
DIR holds `workpiece` and `workpiece-app` (with `.exe` on Windows). Each archive carries the
programs, the two licence files, README.md, CHANGELOG.md, THIRD-PARTY.txt, and the examples
folder if the repository has one.

On macOS, with MACOS_SIGN_IDENTITY set (a "Developer ID Application: ..." identity in the
keychain) the programs and the bundle are signed with the hardened runtime; without it they are
only ad-hoc signed (the app starts, but Gatekeeper asks the user to confirm it once).
"""

import argparse
import os
import plistlib
import shutil
import subprocess
import sys
import tarfile
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BUNDLE_ID = "io.github.geop-cad.workpiece"


def copy_docs(stage: str, third_party: str) -> None:
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "README.md", "CHANGELOG.md"):
        shutil.copy(os.path.join(ROOT, name), stage)
    shutil.copy(third_party, os.path.join(stage, "THIRD-PARTY.txt"))
    examples = os.path.join(ROOT, "examples")
    if os.path.isdir(examples):
        shutil.copytree(examples, os.path.join(stage, "examples"),
                        ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "out", "*.views", "TOOL_FAULTS.md"))


def run(*cmd: str) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.check_call(cmd)


def icns(png: str, out: str) -> None:
    iconset = out[:-5] + ".iconset"
    os.makedirs(iconset, exist_ok=True)
    for size in (16, 32, 64, 128, 256, 512):
        run("sips", "-z", str(size), str(size), png, "--out", os.path.join(iconset, f"icon_{size}x{size}.png"))
        if size > 16:
            half = size // 2
            shutil.copy(os.path.join(iconset, f"icon_{size}x{size}.png"),
                        os.path.join(iconset, f"icon_{half}x{half}@2x.png"))
    run("iconutil", "-c", "icns", iconset, "-o", out)
    shutil.rmtree(iconset)


def macos_app(stage: str, bin_dir: str, version: str) -> str:
    app = os.path.join(stage, "Workpiece.app")
    macos, resources = os.path.join(app, "Contents", "MacOS"), os.path.join(app, "Contents", "Resources")
    os.makedirs(macos)
    os.makedirs(resources)
    for name in ("workpiece-app", "workpiece"):
        shutil.copy(os.path.join(bin_dir, name), macos)
        os.chmod(os.path.join(macos, name), 0o755)
    icns(os.path.join(ROOT, "hosts", "app", "assets", "workpiece.png"), os.path.join(resources, "workpiece.icns"))
    plist = {
        "CFBundleName": "Workpiece",
        "CFBundleDisplayName": "Workpiece",
        "CFBundleIdentifier": BUNDLE_ID,
        "CFBundleExecutable": "workpiece-app",
        "CFBundleIconFile": "workpiece",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version.split("-")[0],
        "CFBundleVersion": version.split("-")[0],
        "NSHighResolutionCapable": True,
        "LSMinimumSystemVersion": "11.0",
        "CFBundleDocumentTypes": [{
            "CFBundleTypeName": "Workpiece part",
            "CFBundleTypeExtensions": ["workpiece"],
            "CFBundleTypeRole": "Editor",
        }],
    }
    with open(os.path.join(app, "Contents", "Info.plist"), "wb") as f:
        plistlib.dump(plist, f)
    identity = os.environ.get("MACOS_SIGN_IDENTITY")
    if identity:
        for name in ("workpiece", "workpiece-app"):
            run("codesign", "--force", "--options", "runtime", "--timestamp", "--sign", identity, os.path.join(macos, name))
        run("codesign", "--force", "--options", "runtime", "--timestamp", "--sign", identity, app)
    else:
        run("codesign", "--force", "--deep", "--sign", "-", app)
    return app


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("platform", choices=["linux-x86_64", "windows-x86_64", "macos-arm64", "macos-x86_64"])
    ap.add_argument("--bin", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--third-party", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    name = f"workpiece-{a.version}-{a.platform}"
    work = os.path.join(a.out, "stage")
    shutil.rmtree(work, ignore_errors=True)
    stage = os.path.join(work, name)
    os.makedirs(stage)
    exe = ".exe" if a.platform.startswith("windows") else ""
    if a.platform.startswith("macos"):
        stage_dmg = os.path.join(work, "dmg")
        os.makedirs(stage_dmg)
        macos_app(stage_dmg, a.bin, a.version)
        os.symlink("/Applications", os.path.join(stage_dmg, "Applications"))
        # The command line beside the app, for a terminal.
        os.makedirs(os.path.join(stage_dmg, "bin"))
        shutil.copy(os.path.join(a.bin, "workpiece"), os.path.join(stage_dmg, "bin"))
        copy_docs(stage_dmg, a.third_party)
        out = os.path.join(a.out, f"Workpiece-{a.version}-{a.platform}.dmg")
        if os.path.exists(out):
            os.remove(out)
        run("hdiutil", "create", "-volname", f"Workpiece {a.version}", "-srcfolder", stage_dmg, "-ov", "-format", "UDZO", out)
        if os.environ.get("MACOS_SIGN_IDENTITY"):
            run("codesign", "--force", "--timestamp", "--sign", os.environ["MACOS_SIGN_IDENTITY"], out)
    else:
        for program in ("workpiece", "workpiece-app"):
            shutil.copy(os.path.join(a.bin, program + exe), stage)
            os.chmod(os.path.join(stage, program + exe), 0o755)
        copy_docs(stage, a.third_party)
        if exe:
            out = os.path.join(a.out, name + ".zip")
            with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
                for folder, _, files in os.walk(stage):
                    for f in files:
                        path = os.path.join(folder, f)
                        z.write(path, os.path.relpath(path, work))
        else:
            out = os.path.join(a.out, name + ".tar.gz")
            with tarfile.open(out, "w:gz") as t:
                t.add(stage, arcname=name)
    shutil.rmtree(work)
    print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
