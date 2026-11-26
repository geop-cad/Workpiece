"""Rebuilds every example and prints one line each: steps ok, mass, drawing problems, seconds.

    python3 examples/build_all.py              # all of them
    python3 examples/build_all.py vise fan     # some, by folder name
    python3 examples/build_all.py --check      # exit with 1 if any example is not clean (CI)

Each example's script builds its `.wpc` file(s) beside itself, with sheets and pictures in
`<file>.wpc.views/` (not tracked). This script then copies what the gallery shows next to
the script: `drawing.pdf` / `drawing.svg` (the example's main sheet: the assembly's, or the part's),
`drawing-<part>.pdf` / `.svg` for the other parts of an assembly, `drawing-<sheet>.pdf` for a second
sheet of a part (a flat pattern), and `thumbnail.png` (the shaded
pictorial of the main file). A clean example has every step ok, every sketch at 0 degrees of
freedom, no lint, and drawings with no problems.

What each `.wpc` file built to (steps, volumes, faces and edges of each body, drawing problems)
is kept in `examples/built.json`: `examples/wasm_check.mjs` builds the files again in the
browser's build of the kernel and holds it to the same (the site's CI runs it before a deploy).
"""
import json, os, re, shutil, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))


def examples() -> list:
    with open(os.path.join(HERE, "index.json")) as f:
        return [e["name"] for e in json.load(f)["examples"]]


def publish(name: str) -> None:
    """Copies the sheets and the thumbnail of a built example next to its script."""
    folder = os.path.join(HERE, name)
    for file in sorted(os.listdir(folder)):
        if not file.endswith(".wpc"):
            continue
        stem = file[: -len(".wpc")]
        views = os.path.join(folder, file + ".views")
        if not os.path.isdir(views):
            continue
        # Each sheet of the file (a drawing step's id): `drawing` of the main file is `drawing.pdf`.
        for sheet in sorted(f[:-4] for f in os.listdir(views) if f.endswith(".pdf")):
            target = "-".join(["drawing"] + ([] if stem == name else [stem]) + ([] if sheet == "drawing" else [sheet]))
            for ext in ("pdf", "svg"):
                shutil.copyfile(os.path.join(views, f"{sheet}.{ext}"), os.path.join(folder, f"{target}.{ext}"))
        if stem == name and os.path.exists(os.path.join(views, "iso.png")):
            thumbnail = os.path.join(folder, "thumbnail.png")
            shutil.copyfile(os.path.join(views, "iso.png"), thumbnail)
            if os.path.getsize(thumbnail) > 150_000:
                # (Kept small for the gallery: 256 colours are plenty for a shaded picture.)
                from PIL import Image
                Image.open(thumbnail).quantize(256).save(thumbnail, optimize=True)


BUILT = os.path.join(HERE, "built.json")


def summary(report: dict) -> dict:
    """What a `--report` says a file built to, as built.json keeps it (wasm_check.mjs makes
    the same of the browser's report)."""
    bodies = [[b["name"], len(b["faces"]), len(b["edges"])] for b in report["bodies"]]
    for placed in report["placed"]:
        bodies += [[f"{placed['name']}/{b['name']}", len(b["faces"]), len(b["edges"])] for b in placed["bodies"]]
    return {
        "steps": len(report["steps"]),
        "failed": report["failed_steps"],
        "volumes": {m["name"]: m["volume"] for m in report["masses"]},
        "bodies": bodies,
        "drawing_problems": sum(len(d["inventory"]["problems"]) for d in report["drawings"]),
    }


def record(name: str) -> None:
    """Keeps in built.json what each `.wpc` file of the example builds to, by the command line."""
    sys.path.insert(0, os.path.join(HERE, "..", "python"))
    from workpiece._bin import command_line
    folder = os.path.join(HERE, name)
    files = {}
    for file in sorted(f for f in os.listdir(folder) if f.endswith(".wpc")):
        with tempfile.TemporaryDirectory() as scratch:
            out = os.path.join(scratch, "report.json")
            subprocess.run([command_line(), "report", file, "--report", out], cwd=folder,
                           capture_output=True, check=False)
            with open(out) as f:
                files[file] = summary(json.load(f))
    built = {}
    if os.path.exists(BUILT):
        with open(BUILT) as f:
            built = json.load(f)
    built[name] = files
    with open(BUILT, "w") as f:
        json.dump({k: built[k] for k in sorted(built)}, f, indent=1)
        f.write("\n")


def build(name: str) -> tuple:
    """Runs the example's script; returns whether it is clean and its line."""
    script = os.path.join(HERE, name, f"{name}.py")
    started = time.time()
    done = subprocess.run([sys.executable, script], capture_output=True, text=True, cwd=os.path.join(HERE, name))
    seconds = time.time() - started
    out = done.stdout.splitlines()
    if done.returncode != 0:
        tail = (done.stderr.strip().splitlines() or ["?"])[-1]
        return False, f"{name:<22} ERROR {tail}  ({seconds:.1f} s)"
    steps = [l for l in out if l.startswith(("ok ", "FAILED"))]
    failed = sum(l.startswith("FAILED") for l in steps)
    free = sum(int(m[1]) for l in steps for m in [re.search(r"dof (\d+)", l)] if m)
    lint = [l for l in out if l.startswith("lint:")]
    total = [l for l in out if l.startswith("mass in all:")]
    if total:
        # (An assembly: what all of it weighs, "at least" where a part is not weighed.)
        m = re.match(r"mass in all: ([\d.]+ g)( \(at least)?", total[-1])
        mass = (m[1] + (" (some parts not weighed)" if m[2] else "")) if m else "?"
    else:
        masses = [l for l in out if l.startswith("mass ")]
        mass = (masses or ["mass ?: ?"])[-1].rsplit(":", 1)[1].strip()
    sheets = [l for l in out if l.startswith("drawing ")]
    problems = [l.split("; ", 1)[1] for l in sheets if not l.endswith("no problems")]
    publish(name)
    record(name)
    clean = done.returncode == 0 and failed == 0 and free == 0 and not lint and not problems
    line = (f"{name:<22} steps {len(steps) - failed}/{len(steps)} ok, dof {free}, lint {len(lint)}, "
            f"mass {mass}, {len(sheets)} sheet(s), "
            f"{'no drawing problems' if not problems else 'PROBLEMS: ' + ' | '.join(problems)}  ({seconds:.1f} s)")
    return clean, line


def main(argv: list) -> int:
    check = "--check" in argv
    names = [a for a in argv if not a.startswith("--")] or examples()
    started, dirty = time.time(), 0
    for name in names:
        clean, line = build(name)
        dirty += not clean
        print(line, flush=True)
    print(f"{len(names)} examples, {dirty} not clean, {time.time() - started:.1f} s in all")
    return 1 if check and dirty else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
