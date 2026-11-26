#!/usr/bin/env python3
"""build_site.py <out dir>: writes the pages of the site into a folder assemble.sh has made.

Written here: index.html (the home page), scripts/<example>.html (each example's script,
highlighted), examples/index.html (every example with its sheets), privacy.html, licenses.html (third-party notices), 404.html, robots.txt and sitemap.xml. Every page but the privacy
page loads analytics.js (anonymous statistics; off unless the deploy set a key). Read at build time, never copied by hand:

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

NAV = [("Drawings", "#drawings"), ("Examples", "#examples"), ("Features", "#features"), ("Scripting", "#scripting"),
       ("Benchmarks", "#benchmarks"), ("Get it", "#get-it")]


def pretty(path):
    """A page's address as Cloudflare Pages serves it: foo.html is served at foo (and redirected there)."""
    return path[:-5] if path.endswith(".html") else path


# The community's chat (the invite does not expire).
DISCORD = "https://discord.gg/btRDgrcwJ5"

def page(title, description, body, root="", path="", extra_head="", script="", base=False, statistics=True):
    """One page. `root` is the way back to the site's top ('' or '../'); `path` its place under SITE_URL."""
    canonical = SITE_URL + pretty(path)
    nav = "".join(f'<a href="{root}{h}">{t}</a>' for t, h in NAV)
    nav += f'<a href="{root}docs/">Documentation</a><a href="{GH}">GitHub</a><a href="{DISCORD}">Discord</a>'
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
{f'<script src="{root}analytics.js" defer></script>' if statistics else ""}
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
    <a href="{BLOB}/CONTRIBUTING.md">Contributing</a> <a href="{GH}/issues">Issues</a> <a href="{DISCORD}">Discord</a>
    <a href="{BLOB}/CHANGELOG.md">Changelog</a> <a href="{root}privacy.html">Privacy</a>
    <a href="{root}licenses.html">Licences</a></p>
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


def sheets(ex):
    """The example's drawing sheets that have an SVG: the main one first, then drawing-<name>.svg sorted.
    Each: svg and pdf (paths under examples/), a label, and the sheet's size in px for width/height."""
    drawing = ex.get("drawing")
    if not drawing:
        return []
    folder = (REPO / "examples" / drawing).parent
    main = folder / (Path(drawing).stem + ".svg")
    files = ([main] if main.exists() else []) + sorted(folder.glob("drawing-*.svg"))
    out = []
    for f in files:
        rel = f.relative_to(REPO / "examples").as_posix()
        box = re.search(r'viewBox="0 0 ([0-9.]+) ([0-9.]+)"', f.read_text()[:400])
        w, h = (float(box.group(1)), float(box.group(2))) if box else (420.0, 297.0)
        pdf = f.with_suffix(".pdf")
        if f == main:
            label = "Assembly" if "assembly" in ex["title"].lower() else "Part"
        else:
            label = f.stem[len("drawing-"):].replace("_", " ").capitalize()
        out.append({"svg": rel, "pdf": pdf.relative_to(REPO / "examples").as_posix() if pdf.exists() else None,
                    "label": label, "key": f.stem, "w": round(w * 4), "h": round(h * 4)})
    return out


def sheet_figure(ex, root="", eager=False):
    """The example's first sheet, large, opening full size on a click; the others as thumbnails that swap in."""
    shs = sheets(ex)
    if not shs:
        return ""
    n, t = ex["name"], ex["title"]
    first = shs[0]
    load = 'loading="eager"' if eager else 'loading="lazy"'

    def attrs(sh):
        return (f'data-svg="{root}examples/{e(sh["svg"])}" data-pdf="{root}examples/{e(sh["pdf"] or "")}" '
                f'data-title="{e(t)}: {e(sh["label"].lower())} sheet" data-sheet-name="{e(sh["key"])}" '
                f'data-open="{root}{app(n, "drawing")}"')
    thumbs = ""
    if len(shs) > 1:
        buttons = "".join(
            f'<button type="button" class="sheet-thumb" aria-pressed="{"true" if i == 0 else "false"}" {attrs(sh)} '
            f'data-w="{sh["w"]}" data-h="{sh["h"]}">'
            f'<img src="{root}examples/{e(sh["svg"])}" alt="" width="{sh["w"]}" height="{sh["h"]}" loading="lazy" decoding="async">'
            f'<span>{e(sh["label"])}</span></button>' for i, sh in enumerate(shs))
        thumbs = f'<div class="sheet-thumbs" role="group" aria-label="{len(shs)} sheets of {e(t)}">{buttons}</div>'
    return f"""<figure class="sheet-fig">
    <button type="button" class="sheet-open" {attrs(first)} aria-label="Open the drawing sheet of {e(t)} full size">
      <img src="{root}examples/{e(first['svg'])}" alt="The ISO drawing sheet of {e(t)}: views with dimensions and a title block" width="{first['w']}" height="{first['h']}" {load} decoding="async">
    </button>
    {thumbs}
  </figure>"""


def card_links(ex, root=""):
    n = ex["name"]
    shs = sheets(ex)
    links = [f'<a href="{root}{app(n)}" class="needs-desktop">Open in the app</a>',
             f'<a href="{root}{app(n, "drawing")}" class="needs-desktop">Drawing in the app</a>']
    if shs and shs[0]["pdf"]:
        links.append(f'<a href="{root}examples/{e(shs[0]["pdf"])}" data-sheet-name="{e(shs[0]["key"])}">PDF</a>')
    if ex.get("script"):
        links.append(f'<a href="{root}scripts/{n}.html">Script</a>')
    return " ".join(links)


def gallery(root=""):
    cards = []
    for ex in EXAMPLES:
        n = ex["name"]
        shs = sheets(ex)
        tags = "".join(f"<li>{e(t)}</li>" for t in ex.get("tags", []))
        cards.append(f"""<article class="card" data-example="{e(n)}">
  <a class="card-main" href="{root}{app(n)}" data-sheet="{root + 'examples/' + e(shs[0]['svg']) if shs else ''}">
    <img src="{root}examples/{e(ex['thumbnail'])}" alt="{e(ex['title'])}: a shaded view of the part" width="640" height="480" loading="lazy" decoding="async">
  </a>
  {sheet_figure(ex, root)}
  <h3><a href="{root}{app(n)}">{e(ex['title'])}</a></h3>
  <p>{e(ex['description'])}</p>
  <ul class="tags" aria-label="Tags">{tags}</ul>
  <p class="card-links">{card_links(ex, root)}</p>
</article>""")
    return "\n".join(cards)


def examples_page():
    """examples/index.html: every example in a row of its own, the render beside its sheets, large."""
    rows = []
    for ex in EXAMPLES:
        n = ex["name"]
        tags = "".join(f"<li>{e(t)}</li>" for t in ex.get("tags", []))
        rows.append(f"""<article class="ex-row" id="{e(n)}" data-example="{e(n)}">
  <div class="ex-side">
    <a class="ex-render" href="../{app(n)}"><img src="{e(ex['thumbnail'])}" alt="{e(ex['title'])}: a shaded view of the part" width="640" height="480" loading="lazy" decoding="async"></a>
    <h2><a href="../{app(n)}">{e(ex['title'])}</a></h2>
    <p>{e(ex['description'])}</p>
    <ul class="tags" aria-label="Tags">{tags}</ul>
    <p class="card-links">{card_links(ex, "../")}</p>
  </div>
  {sheet_figure(ex, "../")}
</article>""")
    body = f"""<div class="wrap examples-page">
  <p class="crumb"><a href="../#examples">&larr; Home</a></p>
  <h1>Examples and their drawings</h1>
  <p class="lead">{len(EXAMPLES)} original parts and assemblies. Each is one Python script that builds the part and its
  ISO drawing sheets; nothing on a sheet was drawn by hand. Click a sheet to see it full size, or open the part in the
  web app, already built.</p>
  {"".join(rows)}
</div>"""
    return page(f"Examples and their drawings – {NAME}", "Every Workpiece example with its ISO drawing sheets, made by the same script as the part.",
                body, root="../", path="examples/", script=LIGHTBOX)


def drawing_feature():
    """Near the top of the home page: one part and its sheet side by side."""
    ex = next((x for x in EXAMPLES if x["name"] == "bearing_housing"), None)
    if not ex or not sheets(ex):
        return ""
    n = ex["name"]
    return f"""<section id="drawings" class="drawings" aria-labelledby="dr-h" data-example="{e(n)}">
  <div class="wrap">
    <h2 id="dr-h">ISO drawings from the same script</h2>
    <p class="lead">The script that builds the part also lays out its sheet: projected and section views, dimensions
    with tolerances, datums, surface texture and the title block. Change a dimension and the drawing follows.</p>
    <div class="dr-row">
      <figure class="dr-part">
        <a href="{app(n)}"><img src="assets/x-bearing_housing.webp" alt="The flanged bearing housing, rendered" width="1200" height="900" loading="lazy" decoding="async"></a>
        <figcaption>{e(ex['title'])}, as built by <a href="scripts/{n}.html">its script</a></figcaption>
      </figure>
      {sheet_figure(ex)}
    </div>
    <p><a class="button" href="examples/">All examples with their drawings</a></p>
  </div>
</section>"""


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
  <p class="actions" data-example="{e(n)}">
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
    text = re.sub(r'out = sys\.argv\[1\] if len\(sys\.argv\) > 1 else "flange.wpc"', 'out = "flange.wpc"', text)
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
    # Best first: by the share of cases passed (the "all" column), ties by the number passed, then by
    # the categories' shares in the table's order; on a full tie Workpiece comes first, then by name.
    def rank(r):
        share = [-(r["per_category"][c]["pass"] / r["per_category"][c]["total"]) for c in cats]
        return (-(r["pass"] / r["cases"]), -r["pass"], *share, r is not sc, r["kernel"].lower())
    for r in sorted(others + [sc], key=rank):
        mine = r is sc
        name = f'<a href="{e(r["link"])}">{e(r["kernel"] if r["kernel"] != "workpiece" else "Workpiece")}</a>'
        if mine:
            name = f'<strong>{name}</strong><br><span class="badge">self-scored, run here {e(r["date"])}, {r["cases"]} cases</span>'
        tds = "".join(f"<td>{cell(r, c)}</td>" for c in cats)
        total = str(r["pass"]) if r["cases"] == others[0]["cases"] else f'{r["pass"]} of {r["cases"]}'
        cls = ' class="mine"' if mine else ""
        trs.append(f'<tr{cls}><th scope="row">{name}</th><td>{e(r["version"])}</td>'
                   f'<td>{e(r["licence"])}</td>{tds}<td><strong>{total}</strong></td></tr>')
    table = f"""<p class="small">Cases passed at 1e-7 mm in brep-conformance, suite {e(sc['suite_version'])}, best first (by the share of cases passed, ties by the number passed). The other rows are the maintainers' reports of
{e(others[0]['date'])} (all {others[0]['cases']} cases, each run by them on one machine); Workpiece's row was run here, self-scored,
{"on the same " + str(sc['cases']) + " cases, the NIST files included" if sc['cases'] == others[0]['cases'] else "on " + str(sc['cases']) + " of those cases, so its cells with another denominator say so"}.</p>
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
        same = f"the same {rep['cases']}" if rep["cases"] == sc["cases"] else f"the {rep['cases']} cases without NIST{apos}s files"
        s1 += f" (the maintainers{apos} run of all {tied[0]['cases']}, and reproduced here on {same})"
    if near:
        s1 += ", and " + ", ".join(f"{r['kernel']} passes {r['pass']} of {r['cases']}" for r in near)
    s1 += f"; the other kernels in the maintainers{apos} runs pass fewer (from {lowest['pass']} of {lowest['cases']} up). "
    s1 += f"On step-conformance, Workpiece is level with {len(level)} published readers at {sstep['pass']} cells"
    if above:
        s1 += ", and " + ", ".join(f"{r['kernel']} passes {r['pass'] - sstep['pass']} more" for r in above)
    sentence = s1 + "."
    if rep["cases"] == sc["cases"]:
        speed = (f"On the same machine, in the same sandbox, springrep’s median per case was {rep['median_ms']} ms and Workpiece’s "
                 f"{sc['median_ms']} ms: neither kernel is faster in every category.")
    else:
        speed = (f"Speed: run back to back in the same sandbox on the {rep['cases']} cases without NIST’s files ({rep['date']}), "
                 f"springrep’s median per case was {rep['median_ms']} ms, lower than Workpiece’s; neither kernel is faster in "
                 "every category (the benchmarks page has the medians).")
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
    <li>{"Every brep-conformance case was run, the 33 that read NIST&rsquo;s STEP files included" if sc['cases'] == others[0]['cases'] else "The brep-conformance cases that read NIST&rsquo;s STEP files were not run"}; Workpiece&rsquo;s adapter does not answer the functions <code>inertia</code>, <code>betti</code>, <code>section_area</code> and <code>mesh</code>, and no case was counted as unsupported for that.</li>
    <li>The boolean bench is Workpiece&rsquo;s own suite, built on public data and brep-bench&rsquo;s cases; the 12 refused STEP files are listed in the method page. Booleans that cannot be done exactly are refused with a reason rather than approximated.</li>
    <li>On step-conformance&rsquo;s shop parts the reference mesh is Workpiece&rsquo;s own, so those cells are a weaker check than the benchmark&rsquo;s. The three cells Workpiece fails are the freeform panel.</li>
    <li>The rows say what each kernel passed in the suite&rsquo;s run, not what it can do outside the suite, and none of the benchmarks measures the sketcher, assemblies, drawings or the app.</li>
  </ul>
  <p>Method, machine, versions, sources, dates and every command to reproduce the numbers:
  <a href="docs/benchmarks.html">the benchmarks page of the documentation</a>
  (<a href="{BLOB}/docs/benchmarks/README.md">source</a>). Last updated {e(RESULTS['generated'])}.</p>
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
            buttons.append(f'<a class="button dl" data-download="{e(d["label"] + " " + d["detail"])}" href="{e(url)}"><strong>{e(d["label"])}</strong><small>{e(d["detail"])}</small></a>')
        else:
            buttons.append(f'<a class="button dl pending" data-download="{e(d["label"] + " " + d["detail"])}" href="{GH}/releases" aria-label="{e(d["label"])}, {e(d["detail"])}: no release yet"><strong>{e(d["label"])}</strong><small>{e(d["detail"])}</small><small class="soon">Coming with the first release</small></a>')
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
        ("f-annotate.webp", "The Annotate tab: a cast bracket with its dimensions, its H7 bore, datum A and a flatness and a parallelism tolerance frame standing on the model in 3D",
         "Dimensions and tolerances on the model",
         ["Dimensions, fits and tolerances (ISO 129-1, ISO 286), datum features (ISO 5459) and geometric tolerances (ISO 1101) are placed on the model in the 3D view, as ISO 16792 shows them, in the Annotate tab or from a script (p.dimension, p.datum_feature, p.geometric_tolerance, tol= on any sketch dimension).",
          "The sizes steps are held to count too: a sketch's dimensions, an extrude's distance, a hole's diameter, each with its tolerance.",
          "The drawing takes them from there: one source for model and sheet. What the model says and a sheet does not show yet, the sheet offers to place."]),
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
  <h3>One script: part, check and drawing</h3>
  <ul><li>A Python script builds the part, its assembly and its ISO drawing sheets; the build reports every step, the
  degrees of freedom left in each sketch, lint and problems on the sheets. See <a href="#scripting">Scripting</a>.</li>
  <li>All {len(EXAMPLES)} <a href="examples/">examples</a> are such scripts, rebuilt clean (every step ok, every sketch fully
  constrained, no drawing problem) by <a href="{BLOB}/examples/build_all.py"><code>examples/build_all.py --check</code></a>.</li>
  <li>Import STEP, IGES, STL and 3MF; export STEP, IGES, STL, 3MF and drawings as PDF and DXF.</li></ul>
</article>""")
    bc = BY_ID["brep-conformance"]
    mine = next(r for r in bc["rows"] if r["kernel"] == "workpiece")
    pc = mine["per_category"]
    cards.append(f"""<article class="feature">
  <div class="mini-stat"><span class="big">{mine['pass']}<span class="of">/{mine['cases']}</span></span>
  <span>cases of the third-party suite brep-conformance {e(mine['suite_version'])} passed at 1e-7 mm, self-scored</span>
  <span class="small">fillets {pc['fillets']['pass']}/{pc['fillets']['total']} · chamfers {pc['chamfers']['pass']}/{pc['chamfers']['total']} · booleans {pc['booleans']['pass']}/{pc['booleans']['total']} · STEP {pc['step']['pass']}/{pc['step']['total']}</span></div>
  <h3>A kernel measured by others&rsquo; tests</h3>
  <ul><li>Booleans, fillets and chamfers on general NURBS edges, shells and STEP import, checked case by case against
  the suite&rsquo;s own expected values; the run, its date and how to repeat it are on <a href="docs/benchmarks.html">the benchmarks page</a>.</li>
  <li>A boolean that cannot be done exactly is refused with a reason, never approximated. See <a href="#benchmarks">Benchmarks</a>.</li></ul>
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
p.build("plate.wpc")
'''


def home():
    ex_note = ""
    # (app/build.sh names it by its contents: workpiece_bg-<hash>.wasm.)
    wasm = next((OUT / "app").glob("workpiece_bg-*.wasm"), None)
    size = f" The first start downloads the program (about {wasm.stat().st_size / 1e6:.0f} MB before compression)." if wasm else ""
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

{drawing_feature()}

<section id="examples" aria-labelledby="ex-h">
  <div class="wrap">
    <h2 id="ex-h">Examples</h2>
    <p class="lead">{len(EXAMPLES)} original parts and assemblies, each written as a Python script that also draws its sheets. Click a part to open it in the web app, already built, or a sheet to see it full size; <a href="examples/">the examples page</a> shows the sheets larger.</p>
    <p class="small mobile-note">The web app needs a desktop browser. On this screen a part's picture opens its drawing sheet instead.</p>
    <div class="grid examples">
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
python -m workpiece flange.wpc   # opens the part in the desktop app</code></pre>
  </div>
</section>

{benchmarks()}

{downloads()}
"""
    return page(f"{NAME}: an open-source CAD kernel and app in Rust", TAGLINE, body, path="", script=LIGHTBOX)


LIGHTBOX = """<dialog id="lightbox" aria-labelledby="lb-title">
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
  // A sheet (.sheet-open) opens full size here; a thumbnail (.sheet-thumb) puts its sheet in the
  // large place of its figure. On a phone a card's picture opens its sheet (the app needs a desktop).
  var box = document.getElementById('lightbox');
  var img = document.getElementById('lb-img'), zoom = document.getElementById('lb-zoom');
  var small = window.matchMedia('(max-width: 820px), (pointer: coarse)');
  var last = null;
  function show(b) {
    if (!box || !box.showModal) { window.location.href = b.dataset.svg; return; }
    last = b;
    var card = b.closest('[data-example]');
    if (card) box.setAttribute('data-example', card.getAttribute('data-example'));
    img.src = b.dataset.svg;
    img.alt = 'Drawing sheet of ' + b.dataset.title;
    img.classList.remove('actual'); zoom.setAttribute('aria-pressed', 'false');
    document.getElementById('lb-title').textContent = b.dataset.title;
    document.getElementById('lb-open').href = b.dataset.open;
    var pdf = document.getElementById('lb-pdf');
    pdf.hidden = !b.dataset.pdf; pdf.href = b.dataset.pdf || '#';
    box.showModal();
  }
  function swap(t) {
    var fig = t.closest('.sheet-fig'), big = fig.querySelector('.sheet-open'), pic = big.querySelector('img');
    ['svg', 'pdf', 'title', 'sheetName'].forEach(function (k) { big.dataset[k] = t.dataset[k]; });
    pic.src = t.dataset.svg; pic.width = +t.dataset.w; pic.height = +t.dataset.h;
    big.setAttribute('aria-label', 'Open ' + t.dataset.title + ' full size');
    fig.querySelectorAll('.sheet-thumb').forEach(function (o) { o.setAttribute('aria-pressed', String(o === t)); });
  }
  document.addEventListener('click', function (ev) {
    var t = ev.target.closest('.sheet-thumb');
    if (t) { swap(t); return; }
    var b = ev.target.closest('.sheet-open');
    if (b) { show(b); return; }
    var a = ev.target.closest('a.card-main');
    if (a && small.matches && a.dataset.sheet) {
      var s = a.parentNode.querySelector('.sheet-open');
      if (s) { ev.preventDefault(); show(s); }
    }
  });
  if (!box) return;
  zoom.addEventListener('click', function () {
    var on = img.classList.toggle('actual'); zoom.setAttribute('aria-pressed', String(on));
  });
  document.getElementById('lb-close').addEventListener('click', function () { box.close(); });
  box.addEventListener('click', function (ev) { if (ev.target === box) box.close(); });
  box.addEventListener('close', function () { img.removeAttribute('src'); if (last) last.focus(); });
})();
</script>
"""


# ---------------------------------------------------------------- privacy

# What analytics.js and the app send. Change this page together with website/analytics.js,
# the app's calls of window.workpieceTrack, and the assistant's functions in cloud/.
PRIVACY_SCRIPT = """<script>
(function () {
  // The opt-out switch: the same flag website/analytics.js reads. One origin serves the site, the
  // book and the app, so it covers all three.
  var KEY = 'workpiece-statistics-off';
  var box = document.getElementById('stats'), note = document.getElementById('stats-note');
  if (navigator.doNotTrack === '1' || window.doNotTrack === '1' || navigator.globalPrivacyControl === true) {
    box.checked = false; box.disabled = true;
    note.textContent = ' Off: your browser asks sites not to track it.';
  } else {
    var off = false;
    try { off = localStorage.getItem(KEY) === '1'; } catch (e) {}
    box.checked = !off;
    box.addEventListener('change', function () {
      try {
        if (box.checked) localStorage.removeItem(KEY); else localStorage.setItem(KEY, '1');
        note.textContent = box.checked ? ' On from the next page you open.' : ' Off. Saved in this browser.';
      } catch (e) {
        note.textContent = ' Your browser blocks storage, so this choice cannot be remembered.';
      }
    });
  }
  // Put together here rather than written out, so a plain scrape of the page does not find the address.
  var user = 'tobi.jacob', domain = 'gmx.net';
  document.querySelectorAll('[data-email]').forEach(function (a) {
    a.href = 'mailto:' + user + '@' + domain; a.textContent = user + '@' + domain;
  });
})();
</script>
"""


def privacy():
    host = SITE_URL.split("//", 1)[-1].rstrip("/")
    body = f"""<div class="wrap prose">
  <h1>Privacy</h1>
  <p class="small">Covers {e(host)}: the home page, the documentation at <a href="docs/">docs/</a>, the examples and
  the application in the browser at <a href="app/">app/</a>. Last changed 26 November 2026.</p>

  <h2>Who is responsible</h2>
  <p>Tobias Jacob<br>Hübingerweg 26<br>56323 Waldesch<br>Germany<br>
  Email: <a data-email href="#">(shown with JavaScript)</a></p>
  <p class="small">This is also the site's imprint (Impressum, § 5 DDG).</p>

  <h2>In short</h2>
  <ul>
    <li>The CAD application runs in your browser. Parts you build and files you open or save stay on your
    computer; nothing is uploaded, unless you use the assistant (below).</li>
    <li>The site and the application count anonymous usage statistics without cookies and without storing
    anything on your device. You can switch them off <a href="#switch">below</a>; they are off by themselves
    if your browser sends Do Not Track or Global Privacy Control.</li>
    <li>No advertising, no tracking across sites, no session recordings, no selling of data. Every script,
    font and picture is served from this site; no page loads anything from someone else's server, except the
    human check of the assistant (Cloudflare Turnstile, below), and only once you write to the assistant.</li>
  </ul>

  <h2>Hosting</h2>
  <p>The site is served by Cloudflare Pages (Cloudflare, Inc., 101 Townsend St, San Francisco, CA 94107, USA). To
  deliver a page, Cloudflare processes your IP address and the technical data of the request (time, address of the
  page, browser identification) and may keep them in short-lived logs for security and to keep the service running.
  Legal basis: Art. 6(1)(f) GDPR, our legitimate interest in serving the site securely. Cloudflare is certified under
  the EU-US Data Privacy Framework and processes this data on our behalf under a data processing agreement.</p>

  <h2 id="statistics">Anonymous usage statistics</h2>
  <p>To learn which pages are read, which examples are opened and which features of the application are used or
  fail, the pages send the following events to PostHog (PostHog Inc.), in PostHog's EU cloud (Frankfurt,
  Germany):</p>
  <ul>
    <li>that a page was opened: its address, the page you came from, browser and version, operating system, device
    type, screen and window size, browser language and time zone;</li>
    <li>on the home page, which sections were on screen long enough to be read;</li>
    <li>which example was opened, and how (in the application, its drawing in the application, or its script);</li>
    <li>which example's drawing sheet was looked at, and how (opened full size, picked among its sheets, or as PDF);</li>
    <li>which kind of link was followed: the application, the documentation, GitHub, PyPI, a download (with the
    platform's name), this page, a heading of the same page, or another;</li>
    <li>in the application: that it started; which built-in example was opened; that a file was opened, saved or
    exported, with its kind or format only (for example “STEP”), never its name or contents; that the assistant was
    opened and that it took a turn, never what was written in it.</li>
  </ul>
  <p>Nothing you type, no file name, no part and no sketch is ever sent. The statistics run in PostHog's cookieless
  mode: no cookie is set and nothing is stored on or read from your device. To count visitors, PostHog computes a
  hash of your IP address and browser identification together with a secret that changes every day; the IP address
  itself is not stored, and the hash cannot be linked across days. PostHog derives an approximate location (country,
  region) from the IP address before discarding it. We do not create user profiles and cannot identify you.
  Statistics are kept for at most one year.</p>
  <p>Legal basis: Art. 6(1)(f) GDPR, our legitimate interest in finding failures and in learning how the site and the
  application are used. You can object at any time with effect for the future with this switch. It covers the whole
  site, the documentation and the application, and is kept in your browser's local storage (a single flag, used for
  nothing else):</p>
  <p id="switch"><label><input type="checkbox" id="stats"> Count my visits to {e(host)}</label>
  <span id="stats-note" class="small"></span></p>
  <p class="small">Statistics are also off if your browser sends Do Not Track or Global Privacy Control, and in any
  build of the site made without a PostHog key (for example one you build yourself).</p>

  <h2 id="assistant">The assistant in the application</h2>
  <p>The application has an assistant that writes and builds parts for you. Opening it loads its scripts and
  Python from this site and asks this site whether the assistant is available; nothing else is sent until you write
  to it. Then:</p>
  <ul>
    <li>Before the first message is sent, your browser loads Cloudflare Turnstile's script from
    <code>challenges.cloudflare.com</code> (Cloudflare, Inc.), which checks that you are a person; it processes your
    IP address and technical data of your browser for that check.</li>
    <li>A session cookie <code>workpiece_agent</code> is set, sent only to this site's assistant service
    (<code>/api/agent</code>) and valid for at most a day. It is strictly necessary for the assistant and holds no
    identifier of you, only a signed session.</li>
    <li>What you write, the scripts the assistant writes and what running them printed, and what the assistant reads
    of the part open in the editor (its file name, its number of steps, the script it was made of, the report of
    building it, its drawing sheets and pictures of it) are sent through our service on Cloudflare to Anthropic
    (Anthropic PBC, USA), whose model writes the answers. Name your file accordingly if its name is personal. Anthropic processes them under its commercial terms for its
    API, which do not allow training models on them. Do not write personal information into the assistant if you do
    not want it sent.</li>
    <li>To share the assistant's daily budget fairly, our service counts turns, and what they cost, per session and
    per IP address (for an IPv6 address, its first half, the /64 network): each session and each address gets a
    small daily allowance of the budget, and the assistant says when it is used up. Those counters are deleted with
    the first request after 00:00 UTC each day. Its log keeps, per turn, token counts and costs only, never a message
    or an address.</li>
  </ul>
  <p>Legal basis: Art. 6(1)(b) GDPR for answering what you asked the assistant, and Art. 6(1)(f) GDPR for protecting
  the service from abuse.</p>

  <h2>Email</h2>
  <p>If you email us, we use your address and message only to answer you, and delete them when the conversation is
  finished unless we are required to keep them. Legal basis: Art. 6(1)(f) GDPR, or Art. 6(1)(b) GDPR where your
  message concerns a contract.</p>

  <h2>Links to other sites</h2>
  <p>Links to GitHub and PyPI open those sites; nothing is sent to them before you click. Their own privacy policies
  apply there.</p>

  <h2>Your rights</h2>
  <p>You have the right of access (Art. 15 GDPR), rectification (Art. 16), erasure (Art. 17), restriction of processing
  (Art. 18), data portability (Art. 20) and to object to processing based on legitimate interests (Art. 21). Because the
  usage statistics are anonymous, we usually cannot find data belonging to you in them. Email us to exercise any of
  these rights. You may also lodge a complaint with a data protection supervisory authority, in particular in the EU
  member state where you live or work. The authority responsible for us is the Landesbeauftragte für den Datenschutz
  und die Informationsfreiheit Rheinland-Pfalz (<a href="https://www.datenschutz.rlp.de">datenschutz.rlp.de</a>).</p>
</div>"""
    return page(f"Privacy – {NAME}", f"What {host} and the application in the browser send, to whom, and how to switch the statistics off.",
                body, path="privacy.html", script=PRIVACY_SCRIPT, statistics=False)


# ---------------------------------------------------------------- licences

def pinned(path, pattern):
    """A version pinned in a build file (vendor.sh, the site workflow), so this page names what is shipped."""
    found = re.search(pattern, (REPO / path).read_text(encoding="utf-8"))
    return found.group(1) if found else "?"


def licenses():
    pyodide = pinned("website/app/vendor.sh", r"(?m)^VERSION=(\S+)")
    posthog = pinned(".github/workflows/site.yml", r'POSTHOG_JS_VERSION: "([^"]+)"')
    body = f"""<div class="wrap prose">
  <h1>Licences</h1>
  <p>{NAME} is licensed under <a href="{BLOB}/LICENSE-MIT">MIT</a> or <a href="{BLOB}/LICENSE-APACHE">Apache-2.0</a>, at
  your option. The application in the browser, and the site, ship the following works of others under their own
  licences.</p>

  <h2>Fonts in the application</h2>
  <p>The application draws its text with the fonts of the crate <code>epaint_default_fonts</code> (part of egui),
  compiled into the application's WebAssembly module (<code>app/workpiece_bg-*.wasm</code>):</p>
  <ul>
    <li><strong>Ubuntu Light</strong>, © 2011 Canonical Ltd., under the Ubuntu Font Licence, Version 1.0.</li>
    <li><strong>Noto Emoji</strong>, © 2013 Google Inc., under the SIL Open Font License, Version 1.1 (OFL-1.1).</li>
    <li><strong>Hack</strong>, © 2018 Source Foundry Authors, under the MIT License; it builds on Bitstream Vera Sans
    Mono, © 2003 Bitstream Inc., under the Bitstream Vera License (reserved font names “Bitstream” and “Vera”), and on
    the DejaVu project's work, which is in the public domain.</li>
    <li><strong>emoji-icon-font</strong>, © 2014 John Slegers, under the MIT License.</li>
  </ul>
  <p>The full texts of these licences, and the list of every Rust crate the application is built from with its
  licence, are in <a href="THIRD-PARTY.txt">THIRD-PARTY.txt</a>.</p>

  <h2>Python in the assistant</h2>
  <p>The assistant runs its scripts in <strong>Pyodide</strong> {e(pyodide)}, served from
  <code>app/assistant/pyodide/</code> unchanged, © the Pyodide contributors, under the
  <a href="https://www.mozilla.org/MPL/2.0/">Mozilla Public License 2.0</a>. Its source is at
  <a href="https://github.com/pyodide/pyodide">github.com/pyodide/pyodide</a> (the release {e(pyodide)}). Pyodide
  contains CPython and its standard library, © Python Software Foundation, under the
  <a href="https://docs.python.org/3/license.html">PSF License Agreement</a>, and further libraries under their own
  licences, listed in <a href="https://github.com/pyodide/pyodide/blob/main/LICENSE">Pyodide's licence notices</a>.</p>

  <h2>Statistics</h2>
  <p>Where the site counts anonymous statistics (see <a href="privacy.html#statistics">Privacy</a>), it serves
  PostHog's browser bundle <code>posthog-js</code> {e(posthog)} from <code>vendor/posthog/</code>, © PostHog Inc. (Hiberly,
  Inc.) and © Mixpanel, Inc., under the licences its package names, Apache-2.0 and MIT
  (<a href="https://github.com/PostHog/posthog-js/blob/main/LICENSE">its licence</a>).</p>
</div>"""
    return page(f"Licences – {NAME}", f"The licences of the fonts, Python and the statistics bundle that {NAME} ships.",
                body, path="licenses.html")


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
    write("privacy.html", privacy())
    write("licenses.html", licenses())
    write("examples/index.html", examples_page())
    urls = ["", "examples/", "app/", "docs/", "docs/benchmarks.html", "privacy.html", "licenses.html"] + scripts
    write("sitemap.xml", '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
          + "".join(f"  <url><loc>{e(SITE_URL + pretty(u))}</loc></url>\n" for u in urls) + "</urlset>\n")
    print(f"build_site: {5 + len(scripts)} pages, version {VERSION}, pip name {PIP}, SITE_URL {SITE_URL}")


if __name__ == "__main__":
    main()
