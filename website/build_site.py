#!/usr/bin/env python3
"""build_site.py <out dir>: writes the pages of the site into a folder assemble.sh has made.

Written here: index.html (the home page), scripts/<example>.html (each example's script,
highlighted), 404.html, robots.txt and sitemap.xml. Read at build time, never copied by hand:

  examples/index.json          the gallery (a card per example)
  docs/benchmarks/results.json the benchmark section
  Cargo.toml                   the version (the names of the release files)
  python/pyproject.toml        the name on PyPI
  website/site.json            the repository, whether a release exists, the file names of a release
                               (.github/workflows/release.yml makes them: change them together)
  $SITE_URL                    where the site is served from (default: site.json's)

Static output only; nothing is loaded from outside the site. The standard library only.
"""
import html
import json
import os
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
OUT = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else sys.exit(__doc__)

CONF = json.loads((HERE / "site.json").read_text())
SITE_URL = os.environ.get("SITE_URL", CONF["site_url"]).rstrip("/") + "/"
GH = CONF["repository"]
BLOB = f"{GH}/blob/{CONF['branch']}"
TREE = f"{GH}/tree/{CONF['branch']}"
NAME = "Workpiece"
TAGLINE = ("An open-source CAD kernel and desktop app, written in Rust: parametric modelling, "
           "a constraint sketcher, assemblies, ISO drawings and scripting.")


def read_version():
    text = (REPO / "Cargo.toml").read_text()
    section = re.search(r"\[workspace\.package\](.*?)(?:\n\[|\Z)", text, re.S).group(1)
    return re.search(r'^version\s*=\s*"([^"]+)"', section, re.M).group(1)


def read_pip_name():
    text = (REPO / "python" / "pyproject.toml").read_text()
    return re.search(r'^name\s*=\s*"([^"]+)"', text, re.M).group(1)


VERSION = read_version()
PIP = read_pip_name()
e = html.escape


def write(rel, text):
    path = OUT / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


# ---------------------------------------------------------------- page frame

NAV = [("Examples", "#examples"), ("Features", "#features"), ("Scripting", "#scripting"),
       ("Benchmarks", "#benchmarks"), ("Get it", "#get-it")]


def page(title, description, body, root="", path="", extra_head="", script="", base=False):
    """One page. `root` is the way back to the site's top ('' or '../'); `path` its place under SITE_URL."""
    canonical = SITE_URL + path
    nav = "".join(f'<a href="{root}{h}">{t}</a>' for t, h in NAV)
    nav += f'<a href="{root}docs/">Documentation</a><a href="{GH}">GitHub</a>'
    base_tag = f'<base href="{SITE_URL}">' if base else ""
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
{base_tag}<title>{e(title)}</title>
<meta name="description" content="{e(description)}">
<meta name="color-scheme" content="light dark">
<link rel="canonical" href="{e(canonical)}">
<link rel="icon" type="image/png" href="{root}assets/favicon.png">
<link rel="apple-touch-icon" href="{root}assets/apple-touch-icon.png">
<meta property="og:type" content="website">
<meta property="og:site_name" content="{NAME}">
<meta property="og:title" content="{e(title)}">
<meta property="og:description" content="{e(description)}">
<meta property="og:url" content="{e(canonical)}">
<meta property="og:image" content="{SITE_URL}assets/og.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="Workpiece: a bracket modelled in the app, beside the name and what the program is">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{e(title)}">
<meta name="twitter:description" content="{e(description)}">
<meta name="twitter:image" content="{SITE_URL}assets/og.png">
<link rel="stylesheet" href="{root}style.css">
{extra_head}</head>
<body>
<a class="skip" href="#main">Skip to the content</a>
<header class="top">
  <div class="wrap">
    <a class="brand" href="{root or './'}"><img src="{root}assets/favicon.png" alt="" width="28" height="28">{NAME}</a>
    <nav aria-label="Main">{nav}</nav>
  </div>
</header>
<main id="main">
{body}
</main>
{footer(root)}
{script}</body>
</html>
"""


def footer(root=""):
    return f"""<footer>
  <div class="wrap">
    <p><strong>{NAME}</strong> is licensed under <a href="{BLOB}/LICENSE-MIT">MIT</a> or
    <a href="{BLOB}/LICENSE-APACHE">Apache-2.0</a>, at your option.</p>
    <p class="links"><a href="{GH}">Repository</a> <a href="{root}docs/">Documentation</a>
    <a href="{BLOB}/CONTRIBUTING.md">Contributing</a> <a href="{GH}/issues">Issues</a>
    <a href="{BLOB}/CHANGELOG.md">Changelog</a></p>
    <p class="small">Open CASCADE, KiCad, FreeCAD, STEP and other names belong to their owners; they are
    named only to identify them. Icons and example models are original to the project.</p>
  </div>
</footer>"""


# ---------------------------------------------------------------- highlighting

PY_KEYWORDS = set("""False None True and as assert async await break class continue def del elif else except
finally for from global if import in is lambda nonlocal not or pass raise return try while with yield""".split())
TOKEN = re.compile(r'''
    (?P<comment>\#[^\n]*)
  | (?P<string>[rbfRBF]{0,2}(?:"""[\s\S]*?"""|\'\'\'[\s\S]*?\'\'\'|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'))
  | (?P<number>\b\d+(?:\.\d*)?(?:[eE][-+]?\d+)?\b)
  | (?P<name>[A-Za-z_][A-Za-z_0-9]*)
''', re.X)


def highlight(code):
    """Python source to HTML spans; keywords, strings, numbers, comments and the names after def/class."""
    out, pos, after_def = [], 0, False
    for m in TOKEN.finditer(code):
        out.append(e(code[pos:m.start()]))
        kind, text = m.lastgroup, m.group()
        if kind == "name":
            if text in PY_KEYWORDS:
                out.append(f'<span class="k">{text}</span>')
                after_def = text in ("def", "class")
                pos = m.end()
                continue
            out.append(f'<span class="f">{text}</span>' if after_def else e(text))
            after_def = False
        else:
            out.append(f'<span class="{ {"comment": "c", "string": "s", "number": "n"}[kind] }">{e(text)}</span>')
        pos = m.end()
    out.append(e(code[pos:]))
    return "".join(out)


def code_block(code, label):
    return f'<pre class="code" tabindex="0" aria-label="{e(label)}"><code>{highlight(code)}</code></pre>'


# ---------------------------------------------------------------- examples

EXAMPLES = json.loads((REPO / "examples" / "index.json").read_text())["examples"]


def app(name, tab=""):
    return f"app/?open={name}" + (f"&amp;tab={tab}" if tab else "")


def gallery():
    cards = []
    for ex in EXAMPLES:
        n = ex["name"]
        folder = ex["file"].split("/")[0]
        drawing = ex.get("drawing")
        svg = re.sub(r"\.pdf$", ".svg", drawing) if drawing else None
        has_svg = svg and (REPO / "examples" / svg).exists()
        tags = "".join(f"<li>{e(t)}</li>" for t in ex.get("tags", []))
        links = [f'<a href="{app(n, "drawing")}" class="needs-desktop">Drawing in the app</a>']
        if has_svg:
            links.insert(0, f'<button type="button" class="sheet" data-svg="examples/{e(svg)}" '
                            f'data-title="{e(ex["title"])}" data-open="{app(n, "drawing")}" '
                            f'data-pdf="examples/{e(drawing)}">View the sheet</button>')
        if drawing:
            links.append(f'<a href="examples/{e(drawing)}">PDF</a>')
        if ex.get("script"):
            links.append(f'<a href="scripts/{n}.html">Script</a>')
        cards.append(f"""<article class="card">
  <a class="card-main" href="{app(n)}" data-sheet="{'examples/' + e(svg) if has_svg else ''}">
    <img src="examples/{e(ex['thumbnail'])}" alt="{e(ex['title'])}: a shaded view of the part" width="640" height="480" loading="lazy" decoding="async">
    <h3>{e(ex['title'])}</h3>
  </a>
  <p>{e(ex['description'])}</p>
  <ul class="tags" aria-label="Tags">{tags}</ul>
  <p class="card-links">{" ".join(links)}</p>
</article>""")
        del folder
    return "\n".join(cards)


def script_pages():
    pages = []
    for ex in EXAMPLES:
        src = ex.get("script")
        if not src or not (REPO / "examples" / src).exists():
            continue
        code = (REPO / "examples" / src).read_text()
        n = ex["name"]
        body = f"""<div class="wrap prose">
  <p class="crumb"><a href="../#examples">&larr; Examples</a></p>
  <h1>{e(ex['title'])}: the script</h1>
  <p>{e(ex['description'])}</p>
  <p class="actions">
    <a class="button" href="../{app(n)}">Open the part in the app</a>
    <a class="button plain" href="../examples/{e(src)}" download>Download the .py</a>
    <a class="button plain" href="{BLOB}/examples/{e(src)}">On GitHub</a>
  </p>
  {code_block(code, ex['title'] + ' script')}
</div>"""
        write(f"scripts/{n}.html", page(f"{ex['title']}: script – {NAME}", ex["description"], body,
                                        root="../", path=f"scripts/{n}.html"))
        pages.append(f"scripts/{n}.html")
    return pages


# ---------------------------------------------------------------- the flange script

def flange_script():
    """agent/examples/flange.py as someone who has pip-installed the package would write it."""
    src = (REPO / "agent" / "examples" / "flange.py").read_text()
    lines = [ln for ln in src.splitlines() if "sys.path.insert" not in ln]
    text = "\n".join(lines) + "\n"
    text = text.replace("import math, os, sys", "import math")
    text = re.sub(r'out = sys\.argv\[1\] if len\(sys\.argv\) > 1 else "flange.workpiece"', 'out = "flange.workpiece"', text)
    text = text.replace("report.show()\n", "").replace('print("volume", report.volume)\n', 'print("volume", report.volume)  # mm^3\n')
    return text


# ---------------------------------------------------------------- benchmarks

RESULTS = json.loads((REPO / "docs" / "benchmarks" / "results.json").read_text())
BY_ID = {b["id"]: b for b in RESULTS["benchmarks"]}


def pct(p, t):
    return 100.0 * p / t if t else 0.0


def bar(label, p, t, note=""):
    full = p == t
    return (f'<li class="bar"><span class="bar-label">{e(label)}</span>'
            f'<span class="bar-track" aria-hidden="true"><span class="bar-fill{"" if full else " partial"}" style="width:{pct(p, t):.1f}%"></span></span>'
            f'<span class="bar-num">{p}/{t}{(" " + e(note)) if note else ""}</span></li>')


def brep_section():
    b = BY_ID["brep-conformance"]
    cats = b["categories"]
    rows = [r for r in b["rows"] if r["origin"] != "reproduced"]
    sc = next(r for r in b["rows"] if r["kernel"] == "workpiece")
    others = [r for r in rows if r is not sc]
    ref = others[0]["per_category"]

    def cell(row, c):
        p, t = row["per_category"][c]["pass"], row["per_category"][c]["total"]
        return str(p) if t == ref[c]["total"] else f"{p} of {t}"

    head = "".join(f'<th scope="col">{c}<br>{ref[c]["total"]}</th>' for c in cats)
    trs = []
    for r in others + [sc]:
        mine = r is sc
        name = f'<a href="{e(r["link"])}">{e(r["kernel"] if r["kernel"] != "workpiece" else "Workpiece")}</a>'
        if mine:
            name = f'<strong>{name}</strong><br><span class="badge">self-scored, run here {e(r["date"])}, {r["cases"]} cases</span>'
        tds = "".join(f"<td>{cell(r, c)}</td>" for c in cats)
        total = f'{r["pass"]} of {r["cases"]}' if mine else str(r["pass"])
        cls = ' class="mine"' if mine else ""
        trs.append(f'<tr{cls}><th scope="row">{name}</th><td>{e(r["version"])}</td>'
                   f'<td>{e(r["licence"])}</td>{tds}<td><strong>{total}</strong></td></tr>')
    table = f"""<p class="small">Cases passed at 1e-7 mm in brep-conformance, suite {e(sc['suite_version'])}. The other rows are the maintainers' reports of
{e(others[0]['date'])} (all {others[0]['cases']} cases, each run by them on one machine); Workpiece's row was run here on {sc['cases']} of those
cases, without the 33 that read NIST's STEP files, so its <em>step</em> and <em>all</em> cells have other denominators.</p>
<div class="table-wrap" tabindex="0" role="region" aria-label="brep-conformance results by kernel">
<table>
<thead><tr><th scope="col">Kernel</th><th scope="col">Version</th><th scope="col">Licence</th>{head}<th scope="col">all<br>{others[0]['cases']}</th></tr></thead>
<tbody>{"".join(trs)}</tbody></table></div>"""
    bars = "".join(bar(c, sc["per_category"][c]["pass"], sc["per_category"][c]["total"]) for c in cats)
    return sc, others, table, bars


def boolean_groups():
    b = BY_ID["boolean-bench"]
    row = b["rows"][0]
    pc = row["per_category"]
    hand = ["box", "cylinder", "sphere", "turned", "coincident", "tangent", "nearmiss", "chain", "random", "features"]
    brep = [k for k in pc if k.startswith("brep-")]
    groups = [("Hand-made cases with closed-form answers", hand),
              (f"brep-bench's cases ({len(brep)} categories)", brep),
              ("Parts of public STEP files: halved, drilled, combined", ["step"]),
              ("675 public STEP files imported as solids", ["step-import"]),
              ("step-conformance cells (below)", ["step-conformance"])]
    out = []
    for label, keys in groups:
        p = sum(pc[k]["pass"] for k in keys)
        t = sum(pc[k]["total"] for k in keys)
        note = ""
        if keys == ["step-import"] and t - p == row["refused"]:
            note = f"({t - p} refused)"
        if keys == ["step-conformance"] and t - p == row["wrong"]:
            note = f"({t - p} wrong)"
        out.append((label, p, t, note))
    assert sum(g[1] for g in out) == row["pass"] and sum(g[2] for g in out) == row["cases"], "boolean groups do not add up"
    return row, out


def step_table():
    b = BY_ID["step-conformance"]
    cats = b["categories"]
    ref = b["rows"][0]["per_category"]
    trs = []
    for r in b["rows"]:
        mine = r["kernel"] == "workpiece"
        label = "Workpiece" if mine else r["kernel"]
        name = f'<a href="{e(r["link"])}">{e(label)}</a>'
        scored = "Workpiece's port of the benchmark's rule (self); not the benchmark's own scorer" if mine else "benchmark maintainers"
        if mine:
            name = f"<strong>{name}</strong>"
        cells = "".join(f'<td>{r["per_category"][c]["pass"]}</td>' for c in cats)
        cls = ' class="mine"' if mine else ""
        trs.append(f'<tr{cls}><th scope="row">{name}</th><td>{e(r["version"])}</td>{cells}'
                   f'<td><strong>{r["pass"]}</strong></td><td>{e(scored)}</td></tr>')
    head = "".join(f'<th scope="col">{c}<br>{ref[c]["total"]}</th>' for c in cats)
    return f"""<p class="small">step-conformance: 27 STEP parts read and meshed at four tolerances, {b['rows'][0]['cases']} cells; cells passed.
Workpiece's row was run here on {e(b['rows'][0]['date'])}; the other readers were run by the benchmark's maintainers on {e(b['rows'][1]['date'])}.</p>
<div class="table-wrap" tabindex="0" role="region" aria-label="step-conformance results by reader">
<table>
<thead><tr><th scope="col">Reader</th><th scope="col">Version</th>{head}<th scope="col">all<br>{b['rows'][0]['cases']}</th><th scope="col">Scored by</th></tr></thead>
<tbody>{"".join(trs)}</tbody></table></div>""", b


def benchmarks():
    sc, others, table, bars = brep_section()
    brow, groups = boolean_groups()
    stable, sb = step_table()
    rep = next(r for r in BY_ID["brep-conformance"]["rows"] if r["origin"] == "reproduced")
    sstep = sb["rows"][0]
    level = [r for r in sb["rows"][1:] if r["pass"] == sstep["pass"]]
    above = [r for r in sb["rows"][1:] if r["pass"] > sstep["pass"]]
    tied = [r for r in others if r["pass"] == r["cases"]]
    near = [r for r in others if r["cases"] > r["pass"] >= r["cases"] - 5]
    lowest = min(others, key=lambda r: r["pass"] / r["cases"])
    n_ahead_total = len(above)
    bool_bars = "".join(bar(l, p, t, n) for l, p, t, n in groups)
    apos = "\u2019"
    names = ", ".join(r["kernel"] for r in tied)
    s1 = f"On brep-conformance, {names} also passes every case"
    if rep["pass"] == rep["cases"]:
        s1 += f" (the maintainers{apos} run of all {tied[0]['cases']}, and reproduced here on the same {rep['cases']})"
    if near:
        s1 += ", and " + ", ".join(f"{r['kernel']} passes {r['pass']} of {r['cases']}" for r in near)
    s1 += f"; the other kernels in the maintainers{apos} runs pass fewer (from {lowest['pass']} of {lowest['cases']} up). "
    s1 += f"On step-conformance, Workpiece is level with {len(level)} published readers at {sstep['pass']} cells"
    if above:
        s1 += ", and " + ", ".join(f"{r['kernel']} passes {r['pass'] - sstep['pass']} more" for r in above)
    sentence = s1 + "."
    speed = (f"On the same machine, in the same sandbox, springrep’s median per case was {rep['median_ms']} ms and Workpiece’s "
             f"{sc['median_ms']} ms: Workpiece is not faster overall, and neither kernel is faster in every category.")
    return f"""<section id="benchmarks" aria-labelledby="bench-h">
<div class="wrap">
  <h2 id="bench-h">Benchmarks</h2>
  <p class="lead">Measured with suites other people wrote or with public data. Every Workpiece number below is
  <strong>self-scored</strong>: run by Workpiece&rsquo;s authors, not by the suites&rsquo; maintainers.
  The table is drawn from <a href="{BLOB}/docs/benchmarks/results.json"><code>results.json</code></a> when the site is built.</p>
  <div class="facts">
    <div class="fact"><span class="big">{sc['pass']}<span class="of">/{sc['cases']}</span></span>
      <span>brep-conformance cases at 1e-7 mm: booleans, constructions, queries, modify, fillets, chamfers, STEP and parts</span></div>
    <div class="fact"><span class="big">{brow['pass']}<span class="of">/{brow['cases']}</span></span>
      <span>cases of the boolean bench: {brow['refused']} refused with a reason, {brow['wrong']} wrong, {brow['crashed']} crashed</span></div>
    <div class="fact"><span class="big">{sstep['pass']}<span class="of">/{sstep['cases']}</span></span>
      <span>step-conformance cells: the STEP importer, scored by Workpiece&rsquo;s port of the rule</span></div>
  </div>
  <div class="bars">
    <div><h3>brep-conformance, by category</h3><ul class="barlist">{bars}</ul></div>
    <div><h3>The boolean bench, by group</h3><ul class="barlist">{bool_bars}</ul></div>
  </div>
  <h3>brep-conformance beside other kernels</h3>
  {table}
  <h3>step-conformance beside other readers</h3>
  {stable}
  <h3>What the data says, and where it stops</h3>
  <ul class="plain">
    <li>{e(sentence)}</li>
    <li>{e(speed)}</li>
    <li>The 33 brep-conformance cases that read NIST&rsquo;s STEP files were not run; Workpiece&rsquo;s adapter does not answer the functions <code>inertia</code>, <code>betti</code>, <code>section_area</code> and <code>mesh</code>.</li>
    <li>The boolean bench is Workpiece&rsquo;s own suite, built on public data and brep-bench&rsquo;s cases; the 12 refused STEP files are listed in the method page. Booleans that cannot be done exactly are refused with a reason rather than approximated.</li>
    <li>On step-conformance&rsquo;s shop parts the reference mesh is Workpiece&rsquo;s own, so those cells are a weaker check than the benchmark&rsquo;s. The three cells Workpiece fails are the freeform panel.</li>
    <li>The rows say what each kernel passed in the suite&rsquo;s run, not what it can do outside the suite, and none of the benchmarks measures the sketcher, assemblies, drawings or the app.</li>
  </ul>
  <p>Method, machine, versions, sources, dates and every command to reproduce the numbers:
  <a href="docs/benchmarks.html">the benchmarks page of the documentation</a>
  (<a href="{BLOB}/docs/benchmarks/README.md">source</a>). Run on {e(RESULTS['generated'])}.</p>
</div>
</section>"""


# ---------------------------------------------------------------- downloads

def downloads():
    released = CONF.get("released", False)
    buttons = []
    for d in CONF["downloads"]:
        fname = d["file"].format(version=VERSION)
        if released:
            url = f"{GH}/releases/download/v{VERSION}/{fname}"
            buttons.append(f'<a class="button dl" href="{e(url)}"><strong>{e(d["label"])}</strong><small>{e(d["detail"])}</small></a>')
        else:
            buttons.append(f'<a class="button dl pending" href="{GH}/releases" aria-label="{e(d["label"])}, {e(d["detail"])}: no release yet"><strong>{e(d["label"])}</strong><small>{e(d["detail"])}</small><small class="soon">Coming with the first release</small></a>')
    note = (f"Version {e(VERSION)}. Each archive holds the programs, the licences and the examples." if released else
            "There is no release yet; these buttons will point at the files of the first one. Until then, build from source (below) or try it in the browser.")
    soon = "" if released else ' <span class="soon-tag">coming with the first release</span>'
    return f"""<section id="get-it" aria-labelledby="get-h">
<div class="wrap">
  <h2 id="get-h">Get it</h2>
  <div class="dl-grid">{"".join(buttons)}</div>
  <p class="small">{note} Release builds are made for all three systems; the authors have so far run the app on Linux only.
  An unsigned build may be stopped by Windows SmartScreen (More info, then Run anyway) or by macOS (right-click, Open).</p>
  <div class="two">
    <div>
      <h3>With pip{soon}</h3>
      <pre class="shell" tabindex="0"><code>pip install {e(PIP)}
python -m workpiece          # the desktop app</code></pre>
      <p class="small">The wheel carries the <code>workpiece</code> command line and the desktop app; the import name is <code>workpiece</code>.</p>
    </div>
    <div>
      <h3>From source</h3>
      <pre class="shell" tabindex="0"><code>git clone {e(GH)}.git
cd Workpiece
cargo run --release -p workpiece-app</code></pre>
      <p class="small">Needs Rust 1.95 or newer; <a href="docs/install.html">the install page</a> lists the system libraries on Linux.</p>
    </div>
  </div>
  <p><a class="button big-button" href="app/">Or try it in your browser</a></p>
</div>
</section>"""


# ---------------------------------------------------------------- the home page

def features():
    items = [
        ("f-modeling.webp", "The Modeling tab: a cast bracket with its steps listed at the left", "Modelling",
         ["Extrude, revolve, sweep and loft; thicken; shell, draft, rib.",
          "Fillet and chamfer on edges of any NURBS shape; holes to the sizes of the standards; threads.",
          "Booleans on tolerant B-reps: one that cannot be done exactly is refused with a reason.",
          "Change an early step and the ones after it are rebuilt."]),
        ("f-sketcher.webp", "A sketch being edited: two circles and six holes with their dimensions", "Constraint sketcher",
         ["Lines, arcs, circles, slots, ellipses, splines, text.",
          "Constraints and driving dimensions with tolerances, solved by the geometric solver; the sketch says how many degrees of freedom are left.",
          "Trim, offset, mirror, patterns; project edges of the model into the sketch."]),
        ("f-drawing.webp", "An ISO drawing sheet of a flanged housing: plan, section and pictorial views with dimensions", "Drawings to ISO",
         ["Projected, section, detail and pictorial views; dimensions with ISO 129-1 tolerances; datums; balloons and parts lists.",
          "Third-angle or first-angle projection; cosmetic threads drawn as ISO 6410 has them.",
          "A drawing is a step of the part, so it follows the part. Export as PDF and DXF."]),
        ("f-assembly.webp", "A brass butt hinge and a caster, both assemblies built in Workpiece", "Assemblies",
         ["Insert part files, STEP files and standard parts (screws, nuts, washers, ball bearings).",
          "Joints with limits and drives; gear, rack and screw mechanisms; drag parts within what the joints allow.",
          "Interference, bill of materials, exploded view."]),
        ("f-sheetmetal.webp", "A sheet-metal cover with vent slots, beside its flat pattern drawn to size", "Sheet metal",
         ["Flange, bend, unfold and flat pattern.",
          "The flat pattern goes on a sheet of its own, dimensioned like any other view."]),
    ]
    cards = []
    for img, alt, title, bullets in items:
        li = "".join(f"<li>{e(b)}</li>" for b in bullets)
        cards.append(f"""<article class="feature">
  <img src="assets/{img}" alt="{e(alt)}" width="900" height="520" loading="lazy" decoding="async">
  <h3>{e(title)}</h3><ul>{li}</ul>
</article>""")
    cards.append(f"""<article class="feature">
  <div class="mini-code">{code_block(MINI, "A short script")}</div>
  <h3>Scripting and exchange</h3>
  <ul><li>A Python library and a command line write and build parts; see <a href="#scripting">Scripting</a>.</li>
  <li>Import STEP, IGES, STL and 3MF; export STEP, IGES, STL, 3MF and drawings as PDF and DXF.</li></ul>
</article>""")
    return "\n".join(cards)


MINI = '''from workpiece import Part

p = Part()
s = p.sketch("XY")
r = s.rect("a", 0, 0, 60, 40)
s.coincident(r.points[0], s.origin)
s.distance_u(r.points[0], r.points[1], 60)
s.distance_v(r.points[1], r.points[2], 40)
p.extrude(s, 10)
p.build("plate.workpiece")
'''


def home():
    ex_note = ""
    wasm = OUT / "app" / "workpiece_bg.wasm"
    size = f" The first start downloads the program (about {wasm.stat().st_size / 1e6:.0f} MB before compression)." if wasm.exists() else ""
    body = f"""<section class="hero">
  <div class="wrap">
    <div class="hero-text">
      <h1>{NAME}</h1>
      <p class="lead">{e(TAGLINE)}</p>
      <p class="actions">
        <a class="button" href="app/">Try it in your browser</a>
        <a class="button plain" href="#get-it">Get it</a>
      </p>
      <p class="small">Early prototype: the part file format and the APIs are not stable. Free, MIT or Apache-2.0. Nothing is uploaded: files you open stay in your browser.{e(size)}</p>
    </div>
    <figure class="hero-shot">
      <img src="assets/hero.webp" alt="The Workpiece app showing a cast aluminium bracket, with the list of steps that built it on the left" width="1600" height="960" fetchpriority="high">
    </figure>
  </div>
</section>

<section id="examples" aria-labelledby="ex-h">
  <div class="wrap">
    <h2 id="ex-h">Examples</h2>
    <p class="lead">{len(EXAMPLES)} original parts and assemblies, each written as a Python script. Click one to open it in the web app, already built.</p>
    <p class="small mobile-note">The web app needs a desktop browser. On this screen a card opens its drawing sheet instead.</p>
    <div class="grid">
{gallery()}
    </div>
  </div>
</section>

<section id="features" aria-labelledby="feat-h">
  <div class="wrap">
    <h2 id="feat-h">What it can do</h2>
    <p class="lead">A part is a list of steps that the program applies in order. The tabs of the app group them; <a href="{BLOB}/Ribbon.md">Ribbon.md</a> lists per feature what is built and what is not yet.</p>
    <div class="grid features">
{features()}
    </div>
  </div>
</section>

<section id="scripting" aria-labelledby="script-h">
  <div class="wrap">
    <h2 id="script-h">Scripting</h2>
    <p class="lead">The part below is made by this script: a dimensioned disc with a bore, a bolt hole patterned six times, a rounded rim, and a drawing. Every sketch is constrained, so the dimensions are the geometry.</p>
    <div class="script-row">
      {code_block(flange_script(), "flange.py")}
      <div class="script-out">
        <figure><img src="assets/s-flange-part.webp" alt="The flange the script makes, in the app" width="680" height="360" loading="lazy" decoding="async"><figcaption>The part</figcaption></figure>
        <figure><img src="assets/s-flange-drawing.webp" alt="The drawing the script adds: the plan and the side view of the flange with its diameters, bolt circle radius and thickness" width="530" height="410" loading="lazy" decoding="async"><figcaption>Its drawing</figcaption></figure>
      </div>
    </div>
    <p>The script is <a href="{BLOB}/agent/examples/flange.py"><code>agent/examples/flange.py</code></a> without its checkout-only path line. The <a href="docs/scripting.html">scripting chapter</a> has the rest; the examples above are longer scripts.</p>
    <pre class="shell" tabindex="0"><code>{"" if CONF.get("released") else "# both coming with the first release" + chr(10)}pip install {e(PIP)}
python -m workpiece flange.workpiece   # opens the part in the desktop app</code></pre>
  </div>
</section>

{benchmarks()}

{downloads()}
"""
    return page(f"{NAME}: an open-source CAD kernel and app in Rust", TAGLINE, body, path="", script=SCRIPT)


SCRIPT = """<dialog id="lightbox" aria-labelledby="lb-title">
  <div class="lb-bar">
    <h2 id="lb-title"></h2>
    <span class="lb-links"><a id="lb-open" href="#" class="needs-desktop">Open in the app</a> <a id="lb-pdf" href="#">PDF</a>
    <button type="button" id="lb-zoom" aria-pressed="false">Actual size</button>
    <button type="button" id="lb-close">Close</button></span>
  </div>
  <div class="lb-body"><img id="lb-img" alt="The drawing sheet"></div>
</dialog>
<script>
(function () {
  var box = document.getElementById('lightbox');
  if (!box || !box.showModal) return;
  var img = document.getElementById('lb-img'), zoom = document.getElementById('lb-zoom');
  var small = window.matchMedia('(max-width: 820px), (pointer: coarse)');
  var last = null;
  function show(svg, title, open, pdf, from) {
    last = from;
    img.src = svg;
    img.alt = 'Drawing sheet of ' + title;
    img.classList.remove('actual'); zoom.setAttribute('aria-pressed', 'false');
    document.getElementById('lb-title').textContent = title;
    document.getElementById('lb-open').href = open;
    document.getElementById('lb-pdf').href = pdf;
    box.showModal();
  }
  document.addEventListener('click', function (ev) {
    var b = ev.target.closest('button.sheet');
    if (b) { show(b.dataset.svg, b.dataset.title, b.dataset.open, b.dataset.pdf, b); return; }
    var a = ev.target.closest('a.card-main');
    if (a && small.matches && a.dataset.sheet) {
      ev.preventDefault();
      var s = a.parentNode.querySelector('button.sheet');
      if (s) show(s.dataset.svg, s.dataset.title, s.dataset.open, s.dataset.pdf, a);
    }
  });
  zoom.addEventListener('click', function () {
    var on = img.classList.toggle('actual'); zoom.setAttribute('aria-pressed', String(on));
  });
  document.getElementById('lb-close').addEventListener('click', function () { box.close(); });
  box.addEventListener('click', function (ev) { if (ev.target === box) box.close(); });
  box.addEventListener('close', function () { img.removeAttribute('src'); if (last) last.focus(); });
})();
</script>
"""


def not_found():
    body = f"""<div class="wrap prose">
  <h1>There is nothing at this address</h1>
  <p>The page may have moved. Start from <a href="{SITE_URL}">the home page</a>, or open <a href="{SITE_URL}docs/">the documentation</a>.</p>
</div>"""
    return page(f"Not found – {NAME}", "This page does not exist.", body, root=SITE_URL, path="404.html", base=True)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    scripts = script_pages()
    write("index.html", home())
    write("404.html", not_found())
    write("robots.txt", f"User-agent: *\nAllow: /\n\nSitemap: {SITE_URL}sitemap.xml\n")
    urls = ["", "app/", "docs/", "docs/benchmarks.html"] + scripts
    write("sitemap.xml", '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
          + "".join(f"  <url><loc>{e(SITE_URL + u)}</loc></url>\n" for u in urls) + "</urlset>\n")
    print(f"build_site: {1 + len(scripts)} pages, version {VERSION}, pip name {PIP}, SITE_URL {SITE_URL}")


if __name__ == "__main__":
    main()
