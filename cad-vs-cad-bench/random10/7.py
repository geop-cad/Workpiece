import os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import datum, Part, axis, topo, material, view, projected, section_view, note
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "7.wpc")
material(here, "1060 ALUM ALLOY", 2700)
p = Part(unit="Millimetre", material="1060 ALUM ALLOY")

# Revolved body: section profile (u = radius, v = height) about the vertical axis.
s = p.sketch("ZX", show_in="secA")
q0 = s.point(0, 0); q1 = s.point(0, 100); q2 = s.point(22, 92); q3 = s.point(22, 85)
q4 = s.point(28, 85); c = s.point(28, 73); q5 = s.point(40, 73); q6 = s.point(40, 0)
l01 = s.line(q0, q1); l12 = s.line(q1, q2); l23 = s.line(q2, q3); l34 = s.line(q3, q4)
arc = s.arc(c, q4, q5, cw=True); l56 = s.line(q5, q6); l60 = s.line(q6, q0)
s.coincident(q0, s.origin)
s.vertical(l01); s.vertical(l23); s.vertical(l56); s.horizontal(l34); s.horizontal(l60)
s.distance_v(q0, q1, 100, on_drawing=False)
s.angle(l12, l34, 20)
s.distance_u(q1, q2, 22, show_in="front", at=(-62, 0))
s.distance_v(q0, q3, 85, on_drawing=False)
s.distance_u(q0, q6, 40, show_in="front", at=(80, 0))
s.tangent(l34, arc); s.tangent(l56, arc)
s.radius(arc, 12)
p.revolve(s, about="u", combine="NewBody", id="body")

# Counterbored hole from the top face.
top = p.plane("Z", 40, id="top")
h = p.sketch(datum(top))
d = h.dot(0, 0); h.coincident(d, h.origin)
p.hole(h, 24, kind="Counterbore", head=50, head_depth=6, id="bore")
p.on_sheet("bore/callout", show_in="secA", at=(-30, 24))

# Scallop through the plate, leaving the 50 wide tab (R90) and the R70 hub; centre on the 240 circle.
b = p.sketch("XY")
ring = b.circle(b.origin, 120, construction=True); b.diameter(ring, 240, at=(-54, 52))
K = b.point(-120, 0); b.point_on(K, ring); b.point_on(K, "u")
A = b.point(-65, 25); B = b.point(-86, 25); C = b.point(-86, -25); D = b.point(-65, -25)
E = b.point(-57, 40.6); F = b.point(-57, -40.6)
lab = b.line(A, B); lcd = b.line(C, D)
tab = b.arc(b.origin, B, C)
r70r = b.arc(b.origin, D, F); r70l = b.arc(b.origin, E, A)
big = b.arc(K, F, E, cw=True)
b.horizontal(lab); b.horizontal(lcd)
bc = b.line(B, C, construction=True); M = b.point(-86, 0); b.point_on(M, "u"); b.midpoint(M, bc)
b.distance_v(B, C, 50, at=(0, 67))
b.radius(tab, 90, at=(59, -35)); b.radius(r70r, 70, at=(52, -10)); b.equal(r70r, r70l); b.radius(big, 75, at=(-25, 58))
p.cut(b, through=True, symmetric=True, id="scallop")

# Same scallop in the raised pad: a full circle cut from the level of the base's top, up through everything.
shelf = p.plane("Z", 22, id="shelf")
a = p.sketch(datum(shelf), on_drawing=False)
a.project("rim", "scallop/start/e22")
k = a.point(-120, 0); a.coincident(k, "rim_center")
circ = a.circle(k, 75); a.equal(circ, "rim")
p.cut(a, through=True, id="padscallop")
p.on_sheet("top", on_drawing=False); p.on_sheet("shelf", on_drawing=False)
rep = p.build(out)
SCALLOP_FACES = ["scallop/side/e22", "padscallop/side/e8"]
rep = p.build(out)
walls = [f for b_ in rep.bodies for f in b_["faces"] if f["name"].startswith(("scallop/", "padscallop/"))]
for f in walls: print(f["name"], round(f["area"]), [round(x,2) for x in f["normal"]], f["flat"])
p.feature_pattern(SCALLOP_FACES, 3, about=axis("Z"))
rep = p.build(out); rep.show()
print("mass g", rep.volume*2.7e-3)

p.dimension("body/ring/p8", kind="Radius", show_in="top", at=(64, 42))
p.dimension("body/ring/p10", kind="Radius", reference=True, show_in="top", at=(64, 29))
p.dimension("body/ring/p10", kind="Diameter", reference=True, show_in="secA", at=(46, 4))
p.dimension("pattern1/1/scallop/side/e22", kind="Diameter", reference=True, show_in="top", at=(-58, -48))
e0 = rep.edges(where=lambda e: e["straight"] and abs(e["from"][2]) < 1e-6 and abs(e["to"][2]) < 1e-6 and e["middle"][0] > 20 and e["middle"][1] > 20)
e22 = rep.edges(where=lambda e: e["straight"] and abs(e["from"][2] - 22) < 1e-6 and abs(e["to"][2] - 22) < 1e-6 and e["middle"][0] > 20 and e["middle"][1] > 20)
p.dimension(e0[0], e22[0], show_in="iso3", reference=True, id="base_h", along=axis("Z"), at=(-45, 0))
sec = p.op("Section", "secA", plane=axis("Y"))
p.drawing(scale=(1, 2), views=[view("top", "Top", (100, 190)), section_view("secA", "secA", (215, 175), of="top", turn=90),
          projected("front", "top", "Below", 100), view("iso", "Isometric", (320, 252), scale=(1, 3)),
          view("iso2", "IsometricBackLeft", (320, 192), scale=(1, 3)), view("iso3", "IsometricBelowFrontRight", (320, 128), scale=(1, 3))],
          notes=[note("top", (70, 66), "CENTER OF R75", to=(-120, 0, 0)),
                 note("iso2", (58, 6), "THIS SHAPE AND CUT", to=(-60, 0, 40)), note("iso2", (58, 1), "PATTERNED 3 TIMES"), note("iso2", (58, -4), "EQUALLY SPACED"),
                 note("iso", (-45, 12), "CYLINDRICAL", to=(85, 0, 31)), note("iso", (-45, -18), "CONICAL", to=(96, 0, 11))])
rep = p.build(out, render=True); rep.show()
print("mass g", rep.volume*2.7e-3)
