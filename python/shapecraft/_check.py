"""What a script declares of the original drawing, and the checks a build can make against it.

    expect = original(views=[("top", "Top"), ("front", "Front"), ("secA", "Section"), ("iso1", "Pictorial")],
                      numbers=[92, 4, 9, 60, "R32", "Ø50", "(9)"], notes=["(4) PLACES"],
                      mass=(450.0, 0.3, "g"), units="mm", decimal=".")
    p.expect(expect)

Written once by whoever first reads the original; later passes and the grader reuse it. The
build then prints a scorecard (views, numbers, notes, mass, problems) and lints the script
(derived numbers, private members, reading its own outputs). Used by the package (`shapecraft/__init__.py`); no need to
import this directly except for `original`.
"""

from __future__ import annotations

import itertools
import re

_declared: list = []  # every original() a script declared: the last one is what lint compares numbers with

KINDS = ("Front", "Back", "Left", "Right", "Top", "Bottom", "Pictorial", "Section", "Detail", "Auxiliary")

_POUNDS = 0.45359237
_UNITS = {"kg": 1.0, "g": 1e-3, "mg": 1e-6, "lb": _POUNDS, "lbs": _POUNDS, "oz": _POUNDS / 16}


class Original:
    """What the original drawing has: views (id, kind), numbers, notes, mass, units."""

    def __init__(self, views, numbers, notes, mass, units, decimal, leading_zero):
        self.views = [(str(i), _kind(k)) for i, k in views]
        self.numbers = list(numbers)
        self.notes = list(notes)
        self.mass = mass
        self.units = units
        self.decimal = decimal
        self.leading_zero = leading_zero if leading_zero is not None else units not in ("in", "inch", "Inch")

    def __repr__(self):
        return f"original(views={self.views}, numbers={self.numbers}, notes={self.notes}, mass={self.mass})"


def _kind(k: str) -> str:
    """A view kind as written (any case; Front/Top/.. Pictorial|Isometric, Section, Detail, Auxiliary)."""
    word = str(k).strip().capitalize()
    if word in ("Isometric", "Iso", "Pictorial", "Dimetric", "Axonometric"):
        return "Pictorial"
    if word in ("Aux", "Auxiliary"):
        return "Auxiliary"
    if word not in KINDS:
        raise ValueError(f"view kind {k!r}: one of {', '.join(KINDS)}")
    return word


def original(views=(), numbers=(), notes=(), mass=None, units: str = "mm", decimal: str = ".",
             leading_zero: bool | None = None) -> Original:
    """The original drawing, as read once.
    views: [(id, kind)] — kind one of Front Back Left Right Top Bottom Pictorial Section Detail
        Auxiliary (the id is yours; kinds are what is compared, ids only when the sheet has the id).
    numbers: every dimension the original prints, one entry per occurrence, as a number or as
        printed with its prefix: 92, 4.5, "R32", "Ø50", "(9)" (a reference), "45°". Not the
        counts of `(4) PLACES` / `4×`: those go to notes.
    notes: words the original prints: "(4) PLACES", "THRU", "CL SYM", "WIDE", one per occurrence.
    mass: (expected, tolerance, unit) e.g. (450.0, 0.3, "g") or (0.785, 0.001, "lb").
    units: "mm" or "in"; decimal "." or ","; leading_zero False for ".75" (default for inches)."""
    return Original(views, numbers, notes, mass, units, decimal, leading_zero)


# --- reading numbers --------------------------------------------------------------------------

_NUMBER = re.compile(r"(?P<pre>[RØ⌀∅ø])?\s*(?P<open>\()?\s*(?P<num>\d*[.,]\d+|\d+)\s*(?P<deg>°)?\s*(?P<close>\))?")
_COUNT = re.compile(r"\(\s*\d+\s*\)\s*PLACES?|\b\d+\s*[×x]\s*", re.I)


def tokens(text: str) -> list:
    """The numbers of a dimension's text as (prefix, value, reference): prefix "" / "R" / "Ø" /
    "°"; counts like `(4) PLACES` and `4×` are not numbers."""
    found = []
    for m in _NUMBER.finditer(_COUNT.sub(" ", text)):
        pre = (m["pre"] or "")
        pre = "Ø" if pre in ("⌀", "∅", "ø") else pre
        if m["deg"]:
            pre = "°"
        found.append((pre, float(m["num"].replace(",", ".")), bool(m["open"] and m["close"])))
    return found


def _wanted(n) -> tuple:
    if isinstance(n, (int, float)):
        return ("", float(n), False)
    t = tokens(str(n))
    if len(t) != 1:
        raise ValueError(f"original numbers: {n!r} is not one number (with an optional R, Ø, ° or brackets)")
    return t[0]


def _show(t) -> str:
    pre, v, ref = t
    s = f"{pre}{v:g}" if pre != "°" else f"{v:g}°"
    return f"({s})" if ref else s


def _same(a, b) -> bool:
    return a[0] == b[0] and abs(a[1] - b[1]) <= 1e-6


# --- the scorecard ------------------------------------------------------------------------------

def _texts(inv: dict):
    dims, notes = [], []
    for v in inv["views"]:
        for it in v["items"]:
            if it["type"] == "dim":
                dims.append(it["text"])
                notes.append(it["text"])
            elif it["type"] == "words":
                notes.append(it["text"])
            elif it["type"] == "line" and it.get("label"):
                notes.append(it["label"])
    return dims, notes


def _norm(s: str) -> str:
    return re.sub(r"\s+", " ", s.replace("/", " ")).strip().upper()


def score(expect: Original, inv: dict, mass_kg, unit: str | None) -> dict:
    """Compares a sheet's inventory (see `Report.sheet`) with what the original has. Returns
    a dict with the numbers behind the scorecard line (`line`) and details per check."""
    out: dict = {}
    # views, by kind
    got = [v["kind"] for v in inv["views"]]
    want = [k for _, k in expect.views]
    left = list(got)
    missing = []
    for k in want:
        if k in left:
            left.remove(k)
        else:
            missing.append(k)
    out["views"] = (len(want) - len(missing), len(want), missing, left)
    wrong_id = []
    for i, k in expect.views:
        have = [v for v in inv["views"] if v["id"] == i]
        if have and have[0]["kind"] != k:
            wrong_id.append(f"{i} is {have[0]['kind']}, not {k}")
    out["view_ids"] = wrong_id
    # numbers
    dims, words = _texts(inv)
    sheet = [t for text in dims for t in tokens(text)]
    pool = list(sheet)
    wanted = [_wanted(n) for n in expect.numbers]
    miss = []
    for w in wanted:
        at = next((k for k, t in enumerate(pool) if _same(w, t)), None)
        if at is None:
            miss.append(w)
        else:
            pool.pop(at)
    out["numbers"] = (len(wanted) - len(miss), len(wanted), [_show(m) for m in miss], [_show(t) for t in pool])
    # notes
    texts = [_norm(t) for t in words]
    wnotes = [_norm(n) for n in expect.notes]
    nmiss = []
    for n in wnotes:
        at = next((k for k, t in enumerate(texts) if n in t), None)
        if at is None:
            nmiss.append(n)
        else:
            texts[at] = texts[at].replace(n, " ", 1)
    out["notes"] = (len(wnotes) - len(nmiss), len(wnotes), nmiss)
    # number format
    fmt = []
    if expect.decimal == "." and any("," in t for t in dims):
        fmt.append("a comma where the original has a decimal point")
    if expect.decimal == "," and any(re.search(r"\d\.\d", t) for t in dims):
        fmt.append("a decimal point where the original has a comma")
    lead = any(re.search(r"(?<![\d.])0[.,]\d", t) for t in dims)
    if not expect.leading_zero and lead:
        fmt.append("leading zeros (0.75) where the original drops them (.75)")
    if expect.leading_zero and any(re.search(r"(?<![\d])[.,]\d", t) for t in dims):
        fmt.append("leading zeros dropped (.75) where the original keeps them (0.75)")
    out["format"] = fmt
    # units
    if expect.units and unit:
        mine = "in" if unit == "Inch" else "mm" if unit == "Millimetre" else unit
        want_u = "in" if expect.units in ("in", "inch", "Inch") else expect.units
        if mine != want_u:
            out["units"] = f"the part is in {mine}, the original in {want_u}"
    # mass
    if expect.mass:
        value, tol, u = expect.mass
        if mass_kg is None:
            out["mass"] = (None, f"unknown (give the part a material of the folder's library)")
        else:
            mine = mass_kg / _UNITS[u]
            out["mass"] = (abs(mine - value) <= tol, f"{mine:.6g} {u} vs {value:g} ± {tol:g}")
    # problems
    off = sum(1 for v in inv["views"] for it in v["items"] if it["type"] == "dim" and any(it.get("ends_off_by") or []))
    out["problems"] = len(inv["problems"])
    out["ends_off"] = off
    out["line"] = scorecard_line(out)
    return out


def scorecard_line(s: dict) -> str:
    a, b, miss, extra = s["views"]
    parts = [f"views {a}/{b}" + (f" (missing {', '.join(miss)})" if miss else "") + (f" (extra {', '.join(extra)})" if extra else "")]
    a, b, miss, extra = s["numbers"]
    parts.append(f"numbers {a}/{b}" + (f" (missing: {', '.join(miss)})" if miss else "") + (f", extra: {', '.join(extra)}" if extra else ""))
    a, b, miss = s["notes"]
    parts.append(f"notes {a}/{b}" + (f" (missing: {', '.join(miss)})" if miss else ""))
    if "mass" in s:
        ok, what = s["mass"]
        parts.append(f"mass {'ok' if ok else 'WRONG' if ok is False else '?'} ({what})")
    parts.append(f"problems {s['problems']}" + (f", ends off geometry {s['ends_off']}" if s["ends_off"] else ""))
    line = "; ".join(parts)
    for k in ("view_ids", "format"):
        if s.get(k):
            line += f"\n  {k.replace('_', ' ')}: " + "; ".join(s[k])
    if s.get("units"):
        line += "\n  units: " + s["units"]
    return line


# --- lint of the script ---------------------------------------------------------------------------

_MINUS, _PLUS = "−", "+"


def _expression(value: float, listed: list):
    """A short expression of listed values that gives `value` (sums, differences of up to three,
    or half of one, or half a sum/difference of two), or None."""
    vals = sorted(set(round(x, 6) for x in listed))
    tol = 1e-6 + 1e-9 * abs(value)

    def close(x):
        return abs(x - value) <= max(tol, 5e-7)

    g = lambda x: f"{x:g}"
    for a in vals:
        if close(a / 2):
            return f"{g(a)} / 2"
        if close(a * 2):
            return f"{g(a)} × 2"
    for a, b in itertools.permutations(vals, 2):
        if close(a + b) and a <= b:
            return f"{g(a)} {_PLUS} {g(b)}"
        if close(a - b):
            return f"{g(a)} {_MINUS} {g(b)}"
        if close((a + b) / 2) and a < b:
            return f"({g(a)} {_PLUS} {g(b)}) / 2"
        if close((a - b) / 2):
            return f"({g(a)} {_MINUS} {g(b)}) / 2"
    for a, b, c in itertools.permutations(vals, 3):
        if close(a - b - c) and b <= c:
            return f"{g(a)} {_MINUS} {g(b)} {_MINUS} {g(c)}"
        if close(a - b + c) and b != c:
            return f"{g(a)} {_MINUS} {g(b)} {_PLUS} {g(c)}"
        if close(a + b + c) and a <= b <= c:
            return f"{g(a)} {_PLUS} {g(b)} {_PLUS} {g(c)}"
    return None


_LENGTH_DIMS = {"distance", "distance_far", "distance_u", "distance_v", "point_line", "radius", "diameter",
                "diameter_about"}


def driving_values(steps: list) -> list:
    """(where, value) of every number the script types as a driving length: sketch dimensions (not
    reference ones), extrude/cut distances, hole diameters and depths, fillet/chamfer/shell sizes."""
    found = []
    for s in steps:
        if "sketch" in s:
            k = s["sketch"]
            options = k.get("dimension_options", {})
            for n, c in enumerate(k["constraints"]):
                if c[0] in _LENGTH_DIMS and not options.get(str(n), {}).get("reference"):
                    found.append((f"sketch {k['id']}: {c[0]} {c[1]}→{c[2] if c[0] not in ('radius', 'diameter') else ''}".rstrip("→"), c[-1]))
        elif "op" in s:
            a, kind, id_ = s["args"], s["op"], s["id"]
            ext = a.get("extent")
            if kind == "Extrude" and ext in ("Distance", "Symmetric"):
                found.append((f"{kind} {id_}: distance", a["distance"]))
            elif kind == "Hole":
                found.append((f"Hole {id_}: diameter", a["diameter"]))
                if ext == "Distance":
                    found.append((f"Hole {id_}: depth", a["depth"]))
            elif kind in ("Fillet",):
                found.append((f"Fillet {id_}: size", a["size"]))
            elif kind == "Chamfer":
                found.append((f"Chamfer {id_}: distance", a["distance"]))
            elif kind == "Shell":
                found.append((f"Shell {id_}: thickness", a["thickness"]))
    return found


def lint_derived(steps: list, expect: Original) -> list:
    """For each typed length that no value of the original's `numbers` has: if it is a sum,
    difference or half of listed values, say so."""
    listed = []
    for n in expect.numbers:
        try:
            t = _wanted(n)
        except ValueError:
            continue
        if t[0] != "°":
            listed.append(t[1])
            if t[0] == "Ø":
                listed.append(t[1] / 2)
            if t[0] == "R":
                listed.append(t[1] * 2)
    out = []
    seen = set()
    for where, value in driving_values(steps):
        if value is None or any(abs(value - x) <= 1e-6 for x in listed) or round(value, 6) in seen:
            continue
        seen.add(round(value, 6))
        how = _expression(value, listed)
        if how:
            out.append(f"the value {value:g} ({where}) is not on the drawing: it looks like {how}; "
                       f"model it from those numbers")
    return out


_READS = re.compile(r"open\(|\.read_text|\.read_bytes|json\.load|os\.path\.exists|os\.listdir|glob\(|Image\.open|\.exists\(")


def lint_source(source: str) -> list:
    """Private members of the layer used (`._x` of what the layer returns), and a script reading
    its own output files (.shapecraft, .views, reports) before its first build."""
    out = []
    lines = source.splitlines()
    first_build = next((n for n, l in enumerate(lines) if ".build(" in l.split("#", 1)[0]), len(lines))
    for n, line in enumerate(lines[:first_build], 1):
        code = line.split("#", 1)[0]
        if _READS.search(code) and re.search(r"\.shapecraft|\.views|report\.json|\bout\b", code):
            out.append(f"line {n}: the script reads its own output before building: build first, and read what `build` returns")
    for n, line in enumerate(lines, 1):
        code = line.split("#", 1)[0]
        m = re.search(r"\b(?!self\b)(\w+)\._([a-z]\w*)", code)
        if m and m[1] not in ("os", "sys", "math", "json", "np", "subprocess") and not code.lstrip().startswith(("import", "from")):
            out.append(f"line {n}: the layer's private member `{m[1]}._{m[2]}` is used: say it with what the layer offers (ask for the feature if it lacks one)")
    return out
