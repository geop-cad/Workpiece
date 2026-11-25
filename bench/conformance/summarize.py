#!/usr/bin/env python3
"""Summarises brep-conformance results files (the JSON `--out` writes).

    python3 -I summarize.py RESULTS.json [RESULTS.json ...]   # tables per kernel
    python3 -I summarize.py --failures RESULTS.json           # every case not at 1e-7, by test

Only reads the files given; prints Markdown.
"""

import collections
import json
import statistics
import sys

TIERS = ["1e-7", "1e-6", "1e-5", "1e-3"]
OTHER = ["fail", "refused", "unsupported", "timeout", "crash"]


def load(path):
    with open(path) as f:
        run = json.load(f)
    return run


def result_of(case):
    return case.get("result") or case.get("outcome")


def table(run):
    cases = run["cases"]
    kernel = run["kernel"]
    suite = run.get("suite")
    suite = suite.get("version") if isinstance(suite, dict) else suite
    print(f"### {kernel['name']} {kernel['version']} (suite {suite}, {len(cases)} cases)\n")
    groups = collections.OrderedDict()
    for case in cases:
        groups.setdefault(case["test"].split("/")[0], []).append(case)
    groups["all"] = cases
    head = ["category", "cases"] + TIERS + OTHER + ["at 1e-7", "at 1e-3 or tighter", "median ms"]
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for name, group in groups.items():
        count = collections.Counter(result_of(c) for c in group)
        tight = count["1e-7"]
        loose = sum(count[t] for t in TIERS)
        times = [c["time_ms"] for c in group if result_of(c) in TIERS and "time_ms" in c]
        median = f"{statistics.median(times):.1f}" if times else "-"
        row = [name, str(len(group))]
        row += [str(count[k]) for k in TIERS + OTHER]
        row += [f"{100 * tight / len(group):.1f}%", f"{100 * loose / len(group):.1f}%", median]
        print("| " + " | ".join(row) + " |")
    print()


def why_of(case):
    why = case.get("why") or case.get("error") or ""
    if not why:
        for op in case.get("ops", []):
            if op.get("outcome") not in ("done", None):
                why = f"{op.get('op')}: {op.get('outcome')}: {op.get('error', '')}"
                break
    return " ".join(str(why).split())


def failures(run):
    by_test = collections.OrderedDict()
    for case in run["cases"]:
        if result_of(case) != "1e-7":
            by_test.setdefault(case["test"], []).append(case)
    totals = collections.Counter(c["test"] for c in run["cases"])
    for test, cases in by_test.items():
        count = collections.Counter(result_of(c) for c in cases)
        said = ", ".join(f"{n} {k}" for k, n in count.most_common())
        print(f"- `{test}` ({len(cases)} of {totals[test]} not at 1e-7: {said})")
        seen = collections.Counter()
        first = {}
        for c in cases:
            key = (result_of(c), why_of(c)[:160])
            seen[key] += 1
            first.setdefault(key, c["id"])
        for (result, why), n in seen.most_common(4):
            print(f"    - {n}x {result}: {why} (e.g. `{first[(result, why)]}`)")
    print()


def main():
    args = sys.argv[1:]
    if args and args[0] == "--failures":
        for path in args[1:]:
            failures(load(path))
        return
    for path in args:
        table(load(path))


if __name__ == "__main__":
    main()
