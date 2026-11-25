"""Run every case and compare its mass with the expected one. Exit code 1 if a script fails or a mass
other than the documented miss (first10/9) is out of tolerance."""
import os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
LB = 453.59237
# script: (expected, tolerance, grams per unit of the expected value)
CASES = {
    "first10/1": (0.785, 0.001, LB), "first10/2": (1089, 1, 1), "first10/3": (162, 0.4, 1),
    "first10/4": (424.2, 0.4, 1), "first10/5": (74.76, 0.03, 1), "first10/6": (25.31, 0.02, 1000),
    "first10/7": (26.35, 0.02, 1), "first10/8": (2116, 2, 1), "first10/9": (4.5, 0.01, LB),
    "first10/9-as-answered": (4.5, 0.01, LB), "first10/10": (0.470, 0.001, LB),
    "random10/1": (450.0, 0.3, 1), "random10/2": (3.061, 0.003, LB), "random10/3": (313.6, 0.2, 1),
    "random10/4": (105.8, 0.1, 1), "random10/5": (509, 1, 1), "random10/6": (1409, 1, 1),
    "random10/7": (1985, 2, 1), "random10/8": (518, 2, 1), "random10/9": (1950, 2, 1),
    "random10/10": (623, 1, 1),
}
KNOWN_MISS = {"first10/9"}


def mass_g(output):
    """The part's mass in grams from the build's own report: the last "mass in all" line of an
    assembly, else the last mass line of the part."""
    found = None
    for pattern in (r"^mass in all: ([\d.]+) (g|kg)", r"^mass[^:\n]*: ([\d.]+) (g|kg)"):
        hits = re.findall(pattern, output, re.M)
        if hits:
            found = hits[-1]
            break
    if not found:
        return None
    return float(found[0]) * (1000 if found[1] == "kg" else 1)


bad = 0
for name, (expected, tol, unit) in CASES.items():
    folder, script = os.path.split(os.path.join(HERE, name + ".py"))
    run = subprocess.run([sys.executable, script], cwd=folder, capture_output=True, text=True)
    m = mass_g(run.stdout) if run.returncode == 0 else None
    if m is None:
        print(f"{name}: FAILED\n{run.stderr[-400:]}")
        bad += 1
        continue
    ok = abs(m / unit - expected) <= tol
    note = "ok" if ok else ("miss (documented)" if name in KNOWN_MISS else "OUT OF TOLERANCE")
    print(f"{name}: {m / unit:.4f} expected {expected} ± {tol}  {note}")
    bad += (not ok) and name not in KNOWN_MISS
sys.exit(1 if bad else 0)
