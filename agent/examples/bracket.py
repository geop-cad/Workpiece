"""Alternator bracket (inch): an example of a script built to a declared original, with a drawing.
Run it, then read out/bracket.wpc.views/sheet.txt and the scorecard it prints."""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))  # a checkout; `pip install workpiece` makes this line unnecessary
from workpiece import Part, material, view, projected, note, original
here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")
os.makedirs(here, exist_ok=True)
out = os.path.join(here, "bracket.wpc")
material(here, "6061 Aluminum", 2700)
p = Part(unit="Inch", material="6061 Aluminum")

# What the original drawing has, read once from its picture (the mass question: 0.XXX lb +/- 0.001).
p.expect(original(
    views=[("front", "Front"), ("top", "Top"), ("bottom", "Bottom"), ("side", "Right"),
           ("iso", "Pictorial"), ("iso2", "Pictorial"), ("iso3", "Pictorial")],
    numbers=[1.25, .75, .875, .75, "(2.75)", 1.125, 1, 1.5, 2.75, .625, "R.50", 5, "R.625", "Ø.5", "Ø.5", "Ø.5", .80, .80],
    notes=["(2) PLACES", "(2) PLACES", "THRU", "CL SYM", "CL SYM", "CL SYM"],
    mass=(0.785, 0.001, "lb"), units="in", decimal="."))

# Front elevation on YZ (u = y along the bracket, v = z up), 1.25 thick (x, both ways about the plane).
s = p.sketch("YZ", id="front")
for n, y, z in [("p0",.5,0),("p1",4.5,0),("p2",5,.5),("p3",5,2.75),("p4",4.25,2.75),("p5",4.25,1),
                ("p6",1.625,1),("p7",1.625,.625),("p8",.75,.625),("p9",.75,2.75),("p10",0,2.75),("p11",0,.5)]:
    s.point(y, z, n)
s.point(.5, .5, "c0"); s.point(4.5, .5, "c1")
a0 = s.arc("c0", "p11", "p0"); a1 = s.arc("c1", "p1", "p2")
lb = s.line("p0","p1"); lr = s.line("p2","p3"); lt1 = s.line("p3","p4"); l5 = s.line("p4","p5")
l6 = s.line("p5","p6"); l7 = s.line("p6","p7"); l8 = s.line("p7","p8"); l9 = s.line("p8","p9")
lt0 = s.line("p9","p10"); ll = s.line("p10","p11")
for l in (lb, l6, l8, lt1, lt0): s.horizontal(l)
for l in (lr, l5, l7, l9, ll): s.vertical(l)
s.tangent(lb, a0); s.tangent(ll, a0); s.tangent(lb, a1); s.tangent(lr, a1)
s.horizontal("p10", "p3"); s.point_on("p0", "u"); s.point_on("p10", "v")
s.radius(a1, .5, decimals=2, at=(96, -46), id="r50"); s.equal(a1, a0)
s.distance_u("p10", "p3", 5, decimals=0)
s.distance_u("p10", "p9", .75, decimals=2)
s.distance_u("p8", "p7", .875)
s.distance_u("p4", "p3", .75, decimals=2)
s.distance_v("p1", "p3", 2.75, decimals=2)
s.distance_v("p1", "p6", 1, decimals=0, at=(8, -22))
s.distance_v("p1", "p8", .625, decimals=3, at=(-33, -31))
p.extrude(s, 1.25, combine="NewBody", symmetric=True, id="body")
p.on_sheet("body", decimals=2)

# End elevation on ZX (u = z, v = x): round top, intersected through; its dot is the cross hole.
t = p.sketch("ZX", id="end", show_in="side")
t.point(0, -.625, "q0"); t.point(0, .625, "q1"); t.point(2.125, .625, "q2"); t.point(2.125, -.625, "q3")
t.point(2.125, 0, "qc"); t.point(2.75, 0, "qt")
ar = t.arc("qc", "q2", "q3", cw=True)
t.line("q0","q1","qb"); t.line("q1","q2","qr"); t.line("q3","q0","ql")
t.vertical("qb"); t.horizontal("qr"); t.horizontal("ql")
t.point_on("q0", "v"); t.point_on("qt", "u"); t.tangent("qr", ar); t.tangent("ql", ar)
t.radius(ar, .625, at=(-24, 52), id="r625"); t.project("top", "body/along/p11"); t.point_on("qt", "top")
t.distance_u("q0", "qt", 2.75, reference=True, show_in="front", decimals=2, at=(-67, 0))
t.construction(t.line("qc","qt","qax")); t.point_on("qt", ar)
t.horizontal("qax")
d = t.dot(2.125, 0, "hole_dot"); t.coincident(d, "qc")
p.extrude(t, 5, combine="Intersect", through=True, symmetric=True, id="round")
p.hole(t, .5, id="cross")
p.on_sheet("cross/callout", show_in="side", at=(30, 32), decimals=1)

# Blind holes up from the bottom, .8 from each end; flat floors as the original draws them.
h = p.sketch("XY", id="hole_a", show_in="bottom")
dd = h.dot(0, .8); h.point_on(dd, "v"); h.distance_v(h.origin, dd, .8, decimals=2, at=(-34, -50))
p.hole(h, .5, depth=1.125, point=180, id="blind_a")
p.on_sheet("blind_a/callout", show_in="bottom", at=(-52, -29))
h = p.sketch("XY", id="hole_b", on_drawing=False)
dd = h.dot(0, 4.2); h.point_on(dd, "v")
e = h.point(0, 5); h.point_on(e, "v"); h.distance_v(h.origin, e, 5, on_drawing=False); h.distance_v(dd, e, .8, decimals=2, show_in="bottom", at=(34, -50))
p.hole(h, .5, depth=1.5, point=180, id="blind_b")
p.on_sheet("blind_b/callout", show_in="bottom", at=(62, -29))

p.drawing(scale=(1, 1), leading_zero=False, centre_lines=["blind_a", "blind_b", {"of": "cross", "view": "side"},
    {"of": "cross", "view": "front", "gaps": [(22, 106)]},
    {"of": "cross", "view": "top", "label": "CL SYM"}, {"of": "cross", "view": "bottom", "label": "CL SYM"}], trim_zeros=True, views=[view("front", "Front", (110, 165), style="HiddenDashed"),
    projected("top", "front", "Above", 75, style="HiddenDashed"), projected("bottom", "front", "Below", 75, style="HiddenDashed"),
    projected("side", "front", "Right", 112, style="HiddenDashed"),
    view("iso", "Isometric", (300, 245), scale=(1, 2)), view("iso2", "IsometricFrontLeft", (350, 165), scale=(1, 2)),
    view("iso3", "IsometricBelowFrontRight", (290, 100), scale=(1, 2))],
    notes=[note(None, (0, 0), "(2) PLACES", dimension="front/r50"), note(None, (0, 0), "(2) PLACES", dimension="end/r625")])
r = p.build(out, render=True); r.show()
print("mass lb", r.mass_total / 0.45359237)
