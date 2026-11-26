"""Flanged bearing housing for a 6205 ball bearing (Ø25 × Ø52 × 15).

A turned hub (a half profile revolved about its drawn axis, dimensioned by diameters) on a square
flange, four counterbored holes for M8 socket head cap screws (one hole and a circular pattern),
fillets where the hub meets the flange and on the flange's corners, chamfers on the seat.
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, axis, note, view, section_view, topo

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "bearing_housing.wpc")
p = Part(material="Cast iron", title="BEARING HOUSING 6205", file="bearing_housing.wpc")

# The hub, turned about z: half its profile on YZ (u = y, v = z), its rotation axis drawn, the
# diameters dimensioned about it. The bore runs through; the bearing seat is 15 deep from the top.
h = p.sketch("YZ", id="hub_profile", show_in="secA")
ax = h.rotation_axis(h.origin, (0, 1))
h.vertical(ax)
a = h.point(17.5, 0, "a")      # bore, on the flange's underside
b = h.point(35, 0, "b")        # the hub's outside
c = h.point(35, 45, "c")
d = h.point(26, 45, "d")       # bearing seat
e = h.point(26, 30, "e")
g = h.point(17.5, 30, "g")
for p0, p1 in ((a, b), (b, c), (c, d), (d, e), (e, g), (g, a)):
    h.line(p0, p1)
h.point_on(a, "u")
h.horizontal(a, b); h.vertical(b, c); h.horizontal(c, d); h.vertical(d, e); h.horizontal(e, g); h.vertical(g, a)
h.diameter_about(a, ax, 35, at=(0, -12))
h.diameter_about(b, ax, 70)
h.diameter_about(d, ax, 52, at=(0, 13), tol="H7")   # H7: bearing outer ring sits in it, sliding fit
h.distance_v(b, c, 45)
h.distance_v(e, d, 15)
p.revolve(h, combine="NewBody", id="hub")
rep = p.build(OUT)

# The flange: 110 square, 14 thick, round the bore (its circle is as large as the hub's bore edge).
bore_edge = rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][2]) < 1e-6
                      and abs(abs(e["middle"][0]) + abs(e["middle"][1]) - 17.5) < 1)
f = p.sketch("XY", id="flange_outline", show_in="top")
f.project("bore", bore_edge[0])
r = f.rect("f", -55, -55, 110, 110)
f.symmetric(r.points[0], r.points[1], "v")
f.symmetric(r.points[1], r.points[2], "u")
f.distance_u(r.points[0], r.points[1], 110)
f.distance_v(r.points[1], r.points[2], 110)
hole = f.circle(f.origin, 17.5, "hole")
f.equal(hole, "bore")
p.extrude(f, 14, id="flange")
rep = p.build(OUT)

# The flange's corners rounded, and the hub run into the flange with a fillet.
corners = rep.edges(where=lambda e: e["straight"] and abs(e["from"][0] - e["to"][0]) < 1e-6
                    and abs(e["from"][1] - e["to"][1]) < 1e-6 and abs(e["from"][0]) > 50)
p.fillet(corners, 12, id="corners")
root = rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][2] - 14) < 1e-6
                 and abs(abs(e["middle"][0]) + abs(e["middle"][1]) - 35) < 1)
p.fillet(root, 5, id="root")
rep = p.build(OUT)
seat = rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][2] - 45) < 1e-6
                 and abs(abs(e["middle"][0]) + abs(e["middle"][1]) - 26) < 1)
p.chamfer(seat, 1, id="seat_chamfer")

# One counterbored hole for an M8 cap screw on the diagonal, then four about the axis.
s = p.sketch(topo("flange/end"), id="hole_place", show_in="top")
dot = s.dot(40, 40, "dot")
s.distance_u(s.origin, dot, 40, tol=0.2)
s.distance_v(s.origin, dot, 40, tol=0.2)
p.hole(s, 9, kind="Counterbore", head=14, head_depth=8.6, id="bolt")
rep = p.build(OUT)
wall = rep.faces(where=lambda f: f["name"].startswith("bolt/") and not f["flat"])[0]
p.feature_pattern([wall], 4, about=axis("Z"), id="bolts")

# Tolerances: the flange's underside is the mounting face, datum A, flat to 0,05; the seat is Ø52 H7
# and the bolt pitch ±0,2 (it locates the mating plate); the casting's rest is DCTG 9.
rep = p.build(OUT)
under = rep.faces(where=lambda f: f["flat"] and f["normal"][2] < -0.99 and abs(f["middle"][2]) < 1e-6)[0]
p.datum_feature("A", on=topo(under), show_in="secA", at=(-14, -32))
p.geometric_tolerance("Flatness", 0.05, on=topo(under), show_in="secA", at=(20, -32))

# The sheet: the plan with the holes, a full section through the axis in line under it, a pictorial.
p.on_sheet("seat_chamfer", on_drawing=False)
p.on_sheet("root", at=(-62, 12))
p.op("Section", id="A", along="Plane", plane=axis("X"), offset=0.0)
p.drawing(views=[view("top", "Top", (115, 205)), section_view("secA", "A", (115, 85), of="top"),
                 view("iso", "Isometric", (315, 190))], scale=(1, 1), format="A3", id="drawing", general_tolerance="ISO 8062-3 DCTG 9",
          centre_lines=["all", {"of": "origin/z", "view": "secA"}],
          notes=[note("secA", (52, 34), "1×45°", to=(0, 26.5, 44.5))])
rep = p.build(OUT, render=True, pictures=True, picture=os.path.join(HERE, "render.png"))
rep.show()
