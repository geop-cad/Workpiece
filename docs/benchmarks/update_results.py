#!/usr/bin/env python3
"""Regenerates docs/benchmarks/results.json from runs.

    python3 -I docs/benchmarks/update_results.py [options]

What it rewrites: the rows that were measured with ShapeCraft's own code in this checkout
(origin "run here") and, if given, the rows of a third-party kernel run in the sandbox
(origin "reproduced"). What it keeps: every row whose origin is "published", which is copied from
the report a benchmark's maintainers published; those are added with --published-conformance.

Inputs (all optional; a benchmark without its input keeps the rows it had):

  --conformance FILE         brep-conformance results of shapecraft (the suite's --out JSON)
  --reproduced FILE...       brep-conformance results of another kernel, run in the sandbox
  --published-conformance FILE...
                             reports the suite's maintainers published (their CDN, see README.md)
  --boolean-log FILE         the standard output of `target/release/boolean-bench`
  --boolean-tsv FILE         target/bench-results/boolean.tsv, written by the same run
  --machine TEXT             the machine the runs were made on, one line
  --commit TEXT              the ShapeCraft commit (and whether the tree was clean)
  --date YYYY-MM-DD          the date of the runs (default: today, UTC)

The benchmark's own names and numbers are written as they come; nothing is rounded or ranked.
"""

import argparse
import collections
import datetime
import json
import os
import re
import statistics
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(HERE, "results.json")

CONFORMANCE_CATEGORIES = ["booleans", "constructions", "queries", "modify", "fillets", "chamfers", "step", "parts"]

# The 27 parts of step-conformance, by kind (its README: 7 shop parts, a freeform panel, 19 of known shape).
SHOP = {"bowl_01", "door_handle_guide", "hinge_jig", "lego_3_pins", "lego_4x2_01", "rail_guide", "sled_base"}
FREEFORM = {"freeform_panel"}

KERNEL_INFO = {
    # name: (licence, link) as the suite's registry (kernels.json) lists them
    "shapecraft": ("MIT OR Apache-2.0", "https://github.com/geop-cad/ShapeCraft"),
    "springrep": ("MIT", "https://github.com/springcraft-ai/springrep"),
    "arris": ("MIT OR Apache-2.0", "https://github.com/arris-labs/arris"),
    "cad-kernel": ("MIT OR Apache-2.0", "https://github.com/bmander/geomsolver"),
    "cadrs": ("MIT OR Apache-2.0", "https://github.com/rvdende/cadrs"),
    "kentron": ("AGPL-3.0-or-later AND BSD-3-Clause AND MIT", "https://github.com/rsz269/kentron"),
    "next.BREP": ("LicenseRef-Autodrop3d", "https://github.com/mmiscool/next.BREP.io_RUST_BREP_KERNEL"),
    "opencascade": ("LGPL-2.1-only WITH OCCT-exception-1.0", "https://github.com/Open-Cascade-SAS/OCCT"),
    "solidcore": ("Apache-2.0", "https://github.com/tangency-labs/solidcore"),
    "anvil-brep": ("MIT OR Apache-2.0", "https://github.com/swperb/anvilcad"),
}


def load(path):
    with open(path) as f:
        return json.load(f)


def conformance_row(report, origin, date, source, note=""):
    """One row of the brep-conformance table from a results file the suite wrote."""
    per = collections.OrderedDict((c, {"pass": 0, "total": 0}) for c in CONFORMANCE_CATEGORIES)
    times = []
    for case in report["cases"]:
        category = case["test"].split("/")[0]
        result = case.get("result") or case.get("outcome")
        cell = per.setdefault(category, {"pass": 0, "total": 0})
        cell["total"] += 1
        if result == "1e-7":
            cell["pass"] += 1
            if "time_ms" in case:
                times.append(case["time_ms"])
    kernel = report["kernel"]
    suite = report["suite"]
    run = report.get("run") or {}
    if isinstance(suite, dict):
        suite_version, suite_commit = suite.get("version"), suite.get("commit")
    else:
        suite_version, suite_commit = suite, None
    wall = report.get("wall_ms") or run.get("wall_ms")
    unix = report.get("unix") or run.get("unix")
    env = report.get("environment") or {}
    machine = env.get("machine") or {}
    where = machine.get("cpu_model") or ""
    licence, link = KERNEL_INFO.get(kernel["name"], ("", ""))
    row = collections.OrderedDict()
    row["kernel"] = kernel["name"]
    row["version"] = kernel["version"]
    row["licence"] = licence
    row["link"] = link
    row["origin"] = origin
    row["scored_by"] = "self" if kernel["name"] == "shapecraft" else "benchmark maintainers" if origin == "published" else "ShapeCraft, from the suite's own report"
    row["suite_version"] = suite_version
    row["suite_commit"] = suite_commit
    row["date"] = datetime.datetime.fromtimestamp(unix, datetime.timezone.utc).strftime("%Y-%m-%d") if unix else date
    row["cases"] = sum(c["total"] for c in per.values())
    row["pass"] = sum(c["pass"] for c in per.values())
    row["per_category"] = per
    row["median_ms"] = round(statistics.median(times), 1) if times else None
    row["wall_s"] = round(wall / 1000, 1) if wall else None
    row["threads"] = env.get("threads")
    row["machine"] = where or None
    row["source"] = source
    row["note"] = note
    return row


def parse_duration_ms(text):
    m = re.fullmatch(r"([0-9.]+)(ns|µs|us|ms|s)", text)
    if not m:
        return None
    return float(m.group(1)) * {"ns": 1e-6, "µs": 1e-3, "us": 1e-3, "ms": 1.0, "s": 1000.0}[m.group(2)]


def boolean_rows(log_path):
    """The per-category table the boolean bench prints, and its total time."""
    cats = collections.OrderedDict()
    total = None
    started = False
    with open(log_path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if parts[:2] == ["category", "cases"]:
                started = True
                continue
            if not started:
                continue
            if not parts:
                break
            if len(parts) != 7:
                continue
            name, cases, ok, refused, wrong, crashed, took = parts
            entry = {
                "pass": int(ok),
                "total": int(cases),
                "refused": int(refused),
                "wrong": int(wrong),
                "crashed": int(crashed),
            }
            if name == "all":
                total = (entry, parse_duration_ms(took))
            else:
                entry["median_ms"] = round(parse_duration_ms(took), 3)
                cats[name] = entry
    return cats, total


def step_conformance_row(tsv_path):
    """ShapeCraft's cells of step-conformance, by kind of part, from the boolean bench's results."""
    kinds = {"shop parts": [0, 0], "freeform panel": [0, 0], "parts of known shape": [0, 0]}
    with open(tsv_path, encoding="utf-8") as f:
        for line in f:
            if not line.startswith("step-conformance/"):
                continue
            case, grade = line.rstrip("\n").split("\t")
            part = case.split("/", 1)[1].split("@")[0]
            kind = "shop parts" if part in SHOP else "freeform panel" if part in FREEFORM else "parts of known shape"
            kinds[kind][1] += 1
            kinds[kind][0] += grade == "pass"
    return collections.OrderedDict((k, {"pass": v[0], "total": v[1]}) for k, v in kinds.items())


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--conformance")
    ap.add_argument("--reproduced", nargs="*", default=[])
    ap.add_argument("--published-conformance", nargs="*", default=[])
    ap.add_argument("--boolean-log")
    ap.add_argument("--boolean-tsv")
    ap.add_argument("--machine", default="")
    ap.add_argument("--commit", default="")
    ap.add_argument("--date", default=datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d"))
    args = ap.parse_args()

    data = load(RESULTS) if os.path.exists(RESULTS) else {}
    benchmarks = {b["id"]: b for b in data.get("benchmarks", [])}

    def bench(id, title, **fields):
        b = benchmarks.setdefault(id, collections.OrderedDict(id=id, title=title, rows=[]))
        b["title"] = title
        b.update(fields)
        return b

    if args.conformance or args.reproduced or args.published_conformance:
        b = bench(
            "brep-conformance",
            "brep-conformance",
            source="https://github.com/springcraft-ai/brep-conformance",
            metric="cases passed at 1e-7 mm, by category",
            categories=CONFORMANCE_CATEGORIES,
        )
        rows = b["rows"]

        def put(row):
            rows[:] = [r for r in rows if (r["kernel"], r["origin"]) != (row["kernel"], row["origin"])]
            rows.append(row)

        if args.conformance:
            put(conformance_row(load(args.conformance), "run here", args.date, "this repository: bench/conformance", f"commit {args.commit}; the cases that read NIST's STEP files (opt-in in the suite) were not run"))
        for path in args.reproduced:
            put(conformance_row(load(path), "reproduced", args.date, "run in the sandbox of bench/conformance", "adapter by the suite's maintainers; built and run offline in the sandbox; the cases that read NIST's STEP files (opt-in in the suite) were not run"))
        for path in args.published_conformance:
            report = load(path)
            run = report["run"]
            put(conformance_row(report, "published", args.date, f"https://cdn.springcraft.ai/brep-conformance/reports/{run['id']}/", f"run {run['id']}, complete: {run['complete']}, verified: {run['verified']}"))
        rows.sort(key=lambda r: (r["kernel"] != "shapecraft", -r["pass"] / max(r["cases"], 1), r["kernel"], r["origin"]))

    if args.boolean_log:
        cats, total = boolean_rows(args.boolean_log)
        b = bench(
            "boolean-bench",
            "The boolean bench",
            source="https://github.com/geop-cad/ShapeCraft/tree/main/bench/boolean",
            metric="cases passed, refused, wrong or crashed",
        )
        row = collections.OrderedDict(
            kernel="shapecraft",
            version=args.commit or "",
            licence=KERNEL_INFO["shapecraft"][0],
            link=KERNEL_INFO["shapecraft"][1],
            origin="run here",
            scored_by="self",
            date=args.date,
            cases=total[0]["total"],
            pass_=total[0]["pass"],
            refused=total[0]["refused"],
            wrong=total[0]["wrong"],
            crashed=total[0]["crashed"],
            per_category=cats,
            wall_s=round(total[1] / 1000, 1),
            machine=args.machine,
            note="",
        )
        row["pass"] = row.pop("pass_")
        b["rows"] = [row]

    if args.boolean_tsv:
        per = step_conformance_row(args.boolean_tsv)
        b = bench(
            "step-conformance",
            "step-conformance",
            source="https://github.com/dan335/step-conformance",
            metric="cells passed (27 parts at four tolerances), by kind of part",
            categories=list(per),
        )
        mine = collections.OrderedDict(
            kernel="shapecraft",
            version=args.commit or "",
            licence=KERNEL_INFO["shapecraft"][0],
            link=KERNEL_INFO["shapecraft"][1],
            origin="run here",
            scored_by="self (ShapeCraft's port of the benchmark's rule, bench/boolean/src/conformance.rs; not the benchmark's scorer)",
            date=args.date,
            cases=sum(c["total"] for c in per.values()),
            per_category=per,
            machine=args.machine,
            note="the shop parts are compared with ShapeCraft's own fine mesh, since the benchmark's reference reader (OpenCascade) is not run here",
        )
        mine["pass"] = sum(c["pass"] for c in per.values())
        b["rows"] = [r for r in b.get("rows", []) if r["origin"] != "run here"] + [mine]
        b["rows"].sort(key=lambda r: (r["origin"] != "run here", -r["pass"], r["kernel"]))

    out = collections.OrderedDict()
    out["schema"] = 1
    out["generated"] = args.date
    out["shapecraft_commit"] = args.commit or data.get("shapecraft_commit", "")
    out["machine"] = args.machine or data.get("machine", "")
    out["benchmarks"] = list(benchmarks.values())
    with open(RESULTS, "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"wrote {RESULTS}", file=sys.stderr)


if __name__ == "__main__":
    main()
