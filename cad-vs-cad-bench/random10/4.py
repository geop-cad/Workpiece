import math, os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import Part, axis, topo, datum, view, projected, note, detail_view, material
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "4.wpc")
material(os.path.dirname(out), "ABS", 1020)
p = Part(material="ABS")
D = 1.02e-3   # g/mm3

# Plan (XY, u = x, v = y): block 25 thick and plate 25 further on to the R22 end, as two areas of one outline
# split at the block's far face; the dia 30 hole is a circle in it. Dimensioned as the plan view is: 76, 25, 25.
t = p.sketch("XY", id="plan", show_in="top")
A = t.point(-22, 26, "A"); B = t.point(22, 26, "B"); M1 = t.point(22, 51, "M1"); M2 = t.point(-22, 51, "M2")
N1 = t.point(22, 76, "N1"); N2 = t.point(-22, 76, "N2"); H = t.point(0, 76, "H")
lAB = t.line(A, B, "lAB"); lBM = t.line(B, M1, "lBM"); lMN = t.line(M1, N1, "lMN")
arc = t.arc(H, N1, N2, "arc"); lNM = t.line(N2, M2, "lNM"); lMA = t.line(M2, A, "lMA"); lSplit = t.line(M2, M1, "lSplit")
t.symmetric(A, B, "v"); t.symmetric(M2, M1, "v"); t.symmetric(N2, N1, "v")
t.vertical(lBM); t.vertical(lMN); t.tangent(arc, lMN)
t.radius(arc, 22)
t.distance_v(t.origin, H, 76, show_in="front")
t.distance_v(M1, H, 25)
t.distance_v(A, M2, 25)
hole = t.circle(H, 15, "hole")
t.diameter(hole, 30, id="hole_dia")
p.extrude(t, 50, combine="NewBody", symmetric=True, areas=[(0, 38)], id="block")   # the block: the area between y = 26 and 51
p.extrude(t, 18, combine="Join", symmetric=True, areas=[(0, 56)], id="plate")   # the plate: the area out to the R22 eye
rep = p.build(out)
edge = rep.edges(where=lambda e: e["straight"] and abs(e["from"][1]-26)<1e-6 and abs(e["to"][1]-26)<1e-6 and abs(e["from"][2]-e["to"][2])>1 and e["middle"][0]>0)[0]

# Origin = centre of the clamp (the dia 30 / R25 circle). The long side runs along +y: the front view (YZ sketch,
# u = y, v = z) shows the C-clamp with the block and the plate to its right; extrusions go along x.
c = p.sketch("YZ", id="clamp", show_in="front")
O = c.origin
# Upper half guessed; the lower half is its mirror about the u axis (v -> -v).
g = {  # (u, v) = (y, z)
 "Pb1": (26, -25), "Q1": (0, -25), "E1": (-13.9, -21.9), "K1": (-14.2, -16.8), "I1": (-14.4, -12.3), "J1": (-3.9, -14.5),
 "J2": (-3.9, 14.5), "I2": (-14.4, 12.3), "K2": (-14.2, 16.8), "E2": (-13.9, 21.9), "Q2": (0, 25), "Pb2": (26, 25)}
for k, (u, v) in g.items():
    c.point(u, v, k)
lTop = c.line("Pb1", "Q1"); lU = c.line("J1", "I1"); lL = c.line("J2", "I2"); lBot = c.line("Q2", "Pb2"); lFace = c.line("Pb2", "Pb1")
aOut1 = c.arc(O, "E1", "Q1"); aTip1 = c.arc("K1", "I1", "E1"); aIn = c.arc(O, "J1", "J2")
aTip2 = c.arc("K2", "E2", "I2"); aOut2 = c.arc(O, "Q2", "E2")
for a, b in (("Pb1", "Pb2"), ("Q1", "Q2"), ("E1", "E2"), ("K1", "K2"), ("I1", "I2"), ("J1", "J2")):
    c.symmetric(a, b, "u")
c.diameter(aIn, 30, at=(-5, -32))
c.radius(aOut1, 25, at=(-56, -44))
c.radius(aTip1, 4, on_drawing=False)
c.tangent(lU, aIn); c.tangent(aTip1, lU); c.tangent(aTip1, aOut1, True)
c.tangent(lTop, aOut1)
c.horizontal(lTop)
c.angle(lU, lL, 30, show_in="detail", at=(-18, 0))
c.project("blk", edge)
c.point_on("Pb1", "blk")
p.extrude(c, 22, combine="Join", symmetric=True, id="clamp_ext")

# The 44 across the block and plate: no sketch carries it (R22 and tangency give it).

p.on_sheet("block", show_in="right")
rep = p.build(out)
tip = rep.faces(near=(0, -17, -18), where=lambda f: not f["flat"] and f["name"].startswith("clamp_ext/"))[0]
p.dimension(tip, kind="Radius", show_in="detail", id="r4", at=(8, -62))
p.on_sheet("plate", show_in="front", at=(25, -30))
p.on_sheet("clamp_ext/distance", show_in="top", at=(-62, 8))
inner = rep.faces(near=(0, 15, 0), where=lambda f: not f["flat"] and f["name"].startswith("clamp_ext/"))[0]
p.dimension("plate/end", datum("XY"), show_in="front", reference=True, id="half_plate", at=(50, 12))
p.dimension("block/start", "block/end", show_in="front", reference=True, id="clamp_h", at=(14, 0))
p.dimension("block/side/e15", "block/side/e16", show_in="right", id="width")
p.dimension("block/side/e15", "block/side/e16", show_in="top", reference=True, id="width_ref")
p.drawing(views=[view("front", "Front", (200, 100), style="HiddenDashed"),
                 projected("top", "front", "Above", 80, style="HiddenDashed"),
                 projected("right", "front", "Right", 110, style="HiddenDashed"),
                 detail_view("detail", "front", (-36.5, 0), 26, (78, 100), scale=(2, 1)),
                 view("iso", "Isometric", (320, 252), scale=(1, 2)), view("iso2", "IsometricBackRight", (375, 200), scale=(1, 2))], scale=(1, 1),
                 notes=[note(None, (0, 0), "(2) PLACES", dimension="r4"), note(None, (0, 0), "THRU", dimension="plan/hole_dia"),
                        note("detail", (-12, 80), "ARC SECTION (not STRAIGHT)", to=(0, -17.1, 14.18)),
                        note("detail", (46, 54), "STRAIGHT SECTION", to=(0, -8.2, 13.33)),
                        note("detail", (46, 36), "TANGENT", to=(0, -12.52, 12.17)),
                        note("iso", (-30, -30), "PLANAR FACE", to=(0, -8.2, 13.33))])
rep = p.build(out, render=True); rep.show()
print("volume", rep.volume, "mass g", rep.volume * D)
print(rep.drawings[0]["problems"])
