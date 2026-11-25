import math, os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import datum, Part, axis, topo, view, projected, note, material
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "5.workpiece")
material(os.path.dirname(out), "Red oak", 570)
p = Part(material="Red oak")
D = 5.7e-4   # 570 kg/m3 in g/mm3

# Origin: bottom of the wall's front face, on the symmetry plane. Shelves run along +y.
# Wall plate: 75 wide (x), 350 high (z), 20 thick going back (-y). ZX sketch: u = z, v = x.
w = p.sketch("ZX", id="wall", show_in="right")
r = w.rect("w", 0, -37.5, 350, 75)
w.point_on(r.points[0], "v")
w.symmetric(r.points[0], r.points[3], "u")
w.distance_u(r.points[0], r.points[1], 350, show_in="front")
w.distance_v(r.points[0], r.points[3], 75, show_in="top")
p.extrude(w, 20, combine="NewBody", flip=True, id="wall_ext")

# One shelf support (YZ sketch: u = y, v = z): 50 high at the wall, 150 long, underside rising 10 degrees
# from the plate's bottom corner. 25 wide, centred.
a = p.sketch("YZ", id="arm", show_in="front")
P2 = a.point(0, 50, "P2"); P3 = a.point(150, 50, "P3"); P4 = a.point(150, 26, "P4")
l23 = a.line(P2, P3); l34 = a.line(P3, P4); l41 = a.line(a.origin, P4); l12 = a.line(a.origin, P2)
a.vertical(l12); a.horizontal(l23); a.vertical(l34)
a.distance_v(a.origin, P2, 50)
a.distance_u(P2, P3, 150)
a.angle("u", l41, 10, at=(40, -55))
p.extrude(a, 25, combine="Join", symmetric=True, id="arm_ext")
rep = p.build(out)
end = rep.edges(where=lambda e: abs(e["from"][1]-150) < 1e-6 and abs(e["to"][1]-150) < 1e-6 and abs(abs(e["from"][0])-12.5) < 1e-6
                and abs(e["from"][0]-e["to"][0]) < 1e-6)
p.fillet(end, 8, id="corners")

# Two holes per shelf: the dia 22 counterbore from the underside up to a floor 25 below the shelf top (a plane 25 off the top face),
# its two centres 75 apart and the far one 50 from the shelf tip (projected from the model); then the dia 10 hole through, from the
# top down, on the counterbore's centres.
rep = p.build(out)
tip = rep.edges(where=lambda e: e["straight"] and abs(e["from"][1] - 150) < 1e-6 and abs(e["to"][1] - 150) < 1e-6 and abs(e["from"][2] - 50) < 1e-6)[0]
fl = p.plane(topo("arm_ext/side/e10"), -25.0, id="cb_floor")
h = p.sketch(datum(fl), id="holes", show_in="front")
h.project("tip", tip)
c1 = h.dot(0, 25); c2 = h.dot(0, 100)
h.point_on(c1, "v"); h.point_on(c2, "v")
h.point_line(c2, "tip", 50)
h.distance_v(c1, c2, 75)
p.hole(h, 22, next=True, id="cbore")
rep = p.build(out)
rims = rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][2] - 25) < 1e-6 and 60 < e["length"] < 75)
t10 = p.sketch(topo("arm_ext/side/e10"), id="thru", on_drawing=False)
t10.project("ra", rims[0]); t10.project("rb", rims[1])
d1 = t10.dot(0, 25); d2 = t10.dot(0, 100)
t10.coincident(d1, "ra_center"); t10.coincident(d2, "rb_center")
p.hole(t10, 10, id="thru10")
# The other two shelves: the arm with its rounded corners and its two holes, 150 apart, in one pattern.
p.feature_pattern(["arm_ext/start", "corners/e0/side/e0", "cbore/side/e1", "cbore/h1/side/e1", "thru10/side/e1", "thru10/h1/side/e1"], 3, along=axis("Z"), spacing=150, id="shelves")
p.on_sheet("shelves", on_drawing=False)
p.on_sheet("cb_floor", on_drawing=False)
p.on_sheet("cbore", on_drawing=False)
p.on_sheet("thru10", on_drawing=False)
p.dimension("shelves/2/arm_ext/side/e10", "shelves/2/cbore/side/e0", show_in="front", id="cb25", at=(64, 63))
p.dimension("shelves/1/cbore/side/e1", kind="Diameter", show_in="front", id="cb_dia", at=(60, 5))
p.dimension("shelves/1/thru10/side/e1", kind="Diameter", show_in="front", id="th_dia", at=(60, -12))

# Numbers no sketch carries: shelf pitch (the lower pair driving, the upper a reference), the counterbore floors below the top, the widths.
p.dimension("arm_ext/side/e10", "shelves/1/arm_ext/side/e10", show_in="front", id="pitch", at=(-34, -20))
p.dimension("shelves/1/arm_ext/side/e10", "shelves/2/arm_ext/side/e10", show_in="front", reference=True, id="pitch_ref", at=(-34, 20))
p.on_sheet("arm_ext", show_in="top")
p.on_sheet("wall_ext", show_in="front", at=(-5, -68))
p.on_sheet("corners", show_in="top", at=(40, -14))
p.dimension("wall_ext/side/e11", "wall_ext/side/e13", show_in="right", reference=True, id="wall_w")
p.dimension("arm_ext/start", "arm_ext/end", show_in="right", reference=True, id="arm_w")

# Sheet: front (the shelf profile), plan above it, side view to the right, a pictorial.
p.drawing(views=[view("front", "Front", (100, 114), style="HiddenDashed"),
                 projected("top", "front", "Above", 128, style="HiddenDashed"),
                 projected("right", "front", "Right", 108, style="HiddenDashed"),
                 view("iso", "Isometric", (265, 205), scale=(1, 3)), view("iso2", "IsometricBelowFrontLeft", (360, 205), scale=(1, 3))], scale=(1, 3),
          notes=[note(None, (0, 0), "(6) PLACES", dimension="corners"),
                 note("front", (72, 88), "TO C'BORE\n(6) PLACES", to=(0, 25, 325)), note(None, (0, 0), "C'BORE (6) PLACES", dimension="cb_dia"), note(None, (0, 0), "HOLE (6) PLACES", dimension="th_dia")])
rep = p.build(out, render=True); rep.show()
print("mass g", rep.volume * D)
print(rep.drawings[0]["problems"])
