"""Axial fan, 160 across: a turned hub and five twisted blades.

The hub is a revolved profile with its bore; one blade is lofted from a section at its root to a
section at its tip, each pitched at its own angle (the twist); a circular feature pattern makes
five. Sections through the blade's root and tip show the two pitch angles.
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, axis, note, profile, view, projected, section_view, datum

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "axial_fan.workpiece")
p = Part(material="ABS", title="AXIAL FAN 160", file="axial_fan.workpiece")

# The hub: half its profile on YZ, turned about z — Ø50 × 30 with a Ø8 bore.
h = p.sketch("YZ", id="hub_profile", show_in="front")
ax = h.rotation_axis(h.origin, (0, 1))
h.vertical(ax)
a, b, c, d = h.point(4, 0, "a"), h.point(25, 0, "b"), h.point(25, 30, "c"), h.point(4, 30, "d")
for p0, p1 in ((a, b), (b, c), (c, d), (d, a)):
    h.line(p0, p1)
h.point_on(a, "u")
h.horizontal(a, b); h.vertical(b, c); h.horizontal(c, d); h.vertical(d, a)
h.diameter_about(a, ax, 8, on_drawing=False)   # (the bore is dimensioned where it is seen)
h.diameter_about(b, ax, 50)
h.distance_v(b, c, 30)
p.revolve(h, combine="NewBody", id="hub")
rep = p.build(OUT)
top_rim = rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][2] - 30) < 1e-6
                    and abs(abs(e["middle"][0]) + abs(e["middle"][1]) - 25) < 1)
p.fillet(top_rim, 5, id="hub_round")


def blade_section(id, offset, length, pitch):
    """A blade section on a plane square to x, `offset` out from the axis: a 3 thick rounded
    strip `length` long between its ends' centres, its middle at the hub's half height, pitched
    `pitch` degrees from the plane of the hub."""
    plane = p.plane("X", offset, id=f"{id}_plane")
    s = p.sketch(datum(plane), id=id, show_in=f"sec_{id}")
    sl = s.slot("b", -length / 2, 15, length, 3)
    c0, c1 = sl.points[0], sl.points[1]
    mid = s.point(0, 15, "mid")
    s.midpoint(mid, s.line(c0, c1, "chord", construction=True))
    s.point_on(mid, "v")
    s.distance_v(s.origin, mid, 15)
    s.angle("u", "chord", pitch)
    return s


# One blade, lofted from its root (inside the hub) to its tip, twisted from 40° to 22°.
root = blade_section("root", 20, 26, 40)
tip = blade_section("tip", 80, 34, 22)
p.loft([root, tip], id="blade", combine="Join")
rep = p.build(OUT)
side = rep.faces(where=lambda f: f["name"].startswith("blade/") and f["area"] > 500)[0]
p.feature_pattern([side], 5, about=axis("Z"), id="blades")

# The sheet: the fan seen along its axis, from the side, a section through the blade's tip (its
# root section is inside the hub: said in a note), and a pictorial.
p.on_sheet("root", on_drawing=False)
bore = rep.faces(where=lambda f: not f["flat"] and f["name"].startswith("hub/") and abs(f["middle"][2] - 15) < 1
                 and abs(f["middle"][0]) + abs(f["middle"][1]) < 5)[0]
p.dimension(bore, kind="Diameter", show_in="top", id="bore", at=(30, 30))
p.on_sheet("root_plane", on_drawing=False)
p.op("Section", id="T", along="Plane", plane=axis("X"), offset=80.0)
p.drawing(views=[view("top", "Top", (125, 190)), projected("front", "top", "Below", 125),
                 section_view("sec_tip", "T", (300, 110), of="top", letter="A", cut_only=True, scale=(2, 1)),
                 view("iso", "Isometric", (320, 225), scale=(1, 2))], scale=(1, 1), format="A3", id="drawing",
          notes=[note("front", (-5, -40), "BLADE ROOT AT 20 FROM THE AXIS: CHORD 26 + R1,5 ENDS, PITCH 40°")])
rep = p.build(OUT, render=True, pictures=True, picture=os.path.join(HERE, "render.png"))
rep.show()
