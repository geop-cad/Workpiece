"""Brass butt hinge: one leaf part placed twice (the second turned end for end, so its knuckles fall
between the first's), a steel pin, and a revolute joint between the leaves.

A leaf is a plate with two knuckles: one knuckle (a tube along the hinge axis) patterned along it,
one relief notch for the other leaf's knuckle patterned the same way, one countersunk screw hole
patterned too. The pin is fastened in the first leaf; the second leaf turns about it.
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, anchor, mate, axis, note, topo, view, projected, section_view, datum

HERE = os.path.dirname(os.path.abspath(__file__))
at = lambda name: os.path.join(HERE, name)
OPEN = 110.0  # how far the hinge is opened, in degrees from folded

# --- The leaf: a 3 thick plate 60 long (y) reaching 35 from the hinge axis (x), flush with the top of
# the knuckles (so two leaves fold face to face); a Ø10 knuckle 15 long, then again 30
# on along the axis.
leaf = Part(material="Brass", title="HINGE LEAF", file="leaf.wpc")
plate_top = leaf.plane("Z", 5, id="plate_top")
leaf.on_sheet("plate_top", on_drawing=False)      # (the top of the Ø10 knuckle)
s = leaf.sketch(datum(plate_top), id="plate_outline", show_in="top")
r = s.rect("p", 0, 0, 35, 60)
s.coincident(r.points[0], s.origin)
s.distance_u(r.points[0], r.points[1], 35)
s.distance_v(r.points[1], r.points[2], 60)
leaf.extrude(s, 3, flip=True, combine="NewBody", id="plate")
k = leaf.sketch("ZX", id="knuckle_section", show_in="secS")
k.diameter(k.circle(k.origin, 5, "outside"), 10)
leaf.extrude(k, 15, id="knuckle")
rep = leaf.build(at("leaf.wpc"))
leaf.feature_pattern([rep.faces(where=lambda f: f["name"].startswith("knuckle/") and not f["flat"])[0]], 2,
                     along=axis("Y"), spacing=30, id="knuckles")
# The relief for the other leaf's knuckle: 6,5 from the axis, from 15 to 30 along it; patterned alike.
n = leaf.sketch("XY", id="relief", show_in="top")
nr = n.rect("n", -6, 15, 12.5, 15)
n.symmetric(nr.points[0], nr.points[1], "v")
n.distance_u(n.origin, nr.points[1], 6.5, at=(-20, 55))
n.distance_v(n.origin, nr.points[1], 15, at=(-45, 68))   # (the strip before the relief, in one chain with its width)
n.distance_v(nr.points[1], nr.points[2], 15, tol=(0.4, 0.2), at=(-15, 68))   # relief 15,2 to 15,4: the other leaf's knuckle (15 +-0,2) never binds
leaf.cut(n, through=True, symmetric=True, id="notch")
rep = leaf.build(at("leaf.wpc"))
leaf.feature_pattern([rep.faces(where=lambda f: f["name"].startswith("notch/") and f["flat"] and abs(f["normal"][0]) > 0.99)[0]],
                     2, along=axis("Y"), spacing=30, id="notches")
# The bore for the pin through both knuckles.
b = leaf.sketch("ZX", id="bore_section", show_in="secS")
b.diameter(b.circle(b.origin, 2.5, "bore"), 5, tol="H7")   # H7 bore, f7 pin: leaf 2 turns on it; leaf 1 holds it staked
leaf.cut(b, through=True, symmetric=True, id="bore")
# A countersunk hole for a wood screw, 22 out and 15 along, and again 30 on.
rep = leaf.build(at("leaf.wpc"))
top = rep.faces(where=lambda f: f["flat"] and f["normal"][2] > 0.99)[0]
h = leaf.sketch(topo(top), id="screw_place", show_in="top")
d = h.dot(22, 15, "s")
h.distance_u(h.origin, d, 22)
h.distance_v(h.origin, d, 15, on_drawing=False)   # (the relief starts at the same 15 from the end)
leaf.hole(h, 4.5, kind="Countersink", head=9, head_angle=90, id="screw")
rep = leaf.build(at("leaf.wpc"))
leaf.feature_pattern([rep.faces(where=lambda f: f["name"].startswith("screw/") and not f["flat"] and f["area"] < 60)[0]],
                     2, along=axis("Y"), spacing=30, id="screws")
leaf.on_sheet("screws/spacing", at=(0, 0))
leaf.on_sheet("bore_section/d8", at=(46, -26))
leaf.on_sheet("screw/callout", at=(-8, -52))
leaf.on_sheet("knuckles", on_drawing=False)
leaf.on_sheet("knuckle", on_drawing=False)           # (as long as the relief starts in)
leaf.on_sheet("notches", on_drawing=False)
leaf.op("Section", id="S", along="Plane", plane=axis("Y"), offset=7.5)
leaf.drawing(views=[view("top", "Top", (130, 185)), section_view("secS", "S", (130, 60), of="top", scale=(2, 1)),
                    view("iso", "Isometric", (320, 200))], scale=(2, 1), format="A3", id="drawing",
             notes=[note("top", (0, -75), "KNUCKLE AND RELIEF REPEAT ONCE, 30 ON ALONG THE AXIS")])

# --- The pin: Ø5 × 60.
pin = Part(material="Steel", title="HINGE PIN", file="pin.wpc", sectioned=False)
p = pin.sketch("ZX", id="pin_section", show_in="front")
p.diameter(p.circle(p.origin, 2.5, "pin"), 5, tol="f7")   # running fit in the H7 knuckle bore
pin.extrude(p, 60, combine="NewBody", id="pin")
prep0 = pin.build(at("pin.wpc"))
shaft = prep0.faces(where=lambda f: not f["flat"])[0]
pin.surface_texture("Ra 0,8", on=topo(shaft), show_in="front", at=(-25, 14))   # running surface in the knuckle bores
pin.drawing(views=[view("front", "Front", (130, 160)),
                   view("iso", "Isometric", (320, 210))], scale=(2, 1), format="A3", id="drawing")

# --- The hinge: the first leaf fixed, the pin fastened in its bore (end to end), the second leaf
# turned end for end and held by a revolute joint where its knuckle meets the first's (at rest it
# folds onto the first, face to face), and driven open to 110°.
lrep, prep = leaf.build(at("leaf.wpc")), pin.build(at("pin.wpc"))
rim = lambda rep, y: rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][1] - y) < 1e-6
                               and abs((e["middle"][0] ** 2 + e["middle"][2] ** 2) ** .5 - 5) < 1e-3)[0]
pin_end = lambda y: prep.edges(where=lambda e: not e["straight"] and abs(e["middle"][1] - y) < 1e-6)[0]
asm = Part(title="BUTT HINGE")
asm.add_part(leaf, id="leaf1")
asm.add_part(pin, id="pin", mates=[mate(anchor(topo(rim(lrep, 0)), "ArcCentre", instance="leaf1"),
                                         anchor(topo(pin_end(0)), "ArcCentre"), "Fastened")])
asm.add_part(leaf, id="leaf2", mates=[mate(anchor(topo(rim(lrep, 45)), "ArcCentre", instance="leaf1"),
                                           anchor(topo(rim(lrep, 15)), "ArcCentre"), "Revolute", flip_with=True)])
asm.op("Drive", "open", joint="leaf2", at=OPEN)
# (Beside the plan, the view along the hinge's axis: how far it is open.)
asm.drawing(views=[view("top", "Top", (110, 190)), projected("side", "top", "Right", 120),
                   view("iso", "Isometric", (335, 225), scale=(1, 1))], scale=(2, 1), format="A3", id="drawing",
            parts_list=True, balloons="auto",
            notes=[note("top", (12, -62), "PIN Ø5 f7 IN KNUCKLE BORES H7, PEENED OVER AT BOTH ENDS")])
rep = asm.build(at("butt_hinge.wpc"), render=True, pictures=True, picture=at("render.png"))
for part in asm.reports.values():
    part.show()
rep.show()
