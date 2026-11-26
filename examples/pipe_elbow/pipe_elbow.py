"""Flanged 90° pipe elbow: a tube swept along a bend, a flange with four bolt holes, and the flange
mirrored onto the other end about the bend's plane of symmetry.

The Ø60/Ø50 ring is swept along an R100 quarter circle; the first flange (its bore drawn against
the pipe's inside) gets one bolt hole, patterned four times; a feature mirror in the plane that
halves the bend makes the second flange with its holes.
"""
import math, os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, axis, datum, view, section_view, topo, note

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "pipe_elbow.wpc")
p = Part(material="Stainless steel", title="ELBOW 90° DN50", file="pipe_elbow.wpc")

# The section: a Ø60 / Ø50 ring on the origin, square to z.
sec = p.sketch("XY", id="section", on_drawing=False)  # (the tube's Ø60 is a note: no view shows the outer circle true)
sec.diameter(sec.circle(sec.origin, 30, "outside"), 60)
sec.diameter(sec.circle(sec.origin, 25, "inside"), 50, show_in="bottom", at=(-32, -10), on_drawing=True)

# The path, on ZX (u = z, v = x): a quarter circle of R100 from the origin, leaving it along z and
# arriving along x; its centre on the v axis.
path = p.sketch("ZX", id="bend", show_in="secA")
centre = path.point(0, 100, "centre")
end = path.point(100, 100, "end")
path.arc(centre, path.origin, end, "bend_arc")
path.point_on(centre, "v")
path.horizontal(centre, end)
path.radius("bend_arc", 100)
p.sweep(sec, path, combine="NewBody", id="pipe")


# The first flange: Ø130 × 12 behind the pipe's start, its bore as large as the pipe's inside.
rep = p.build(OUT)
inside = rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][2]) < 1e-6 and math.hypot(e["middle"][0], e["middle"][1]) < 26)[0]
f = p.sketch("XY", id="flange_outline", show_in="bottom")
f.project("inside", inside)
f.diameter(f.circle(f.origin, 65, "rim"), 130)
bore = f.circle(f.origin, 25, "bore")
f.equal(bore, "inside")
p.extrude(f, 12, flip=True, id="flange")

# One bolt hole on the Ø100 pitch circle at 45°, through the flange, and four about its axis.
rep = p.build(OUT)
back = rep.faces(where=lambda f: f["flat"] and f["normal"][2] < -0.99)[0]
b = p.sketch(topo(back), id="bolt_place", show_in="bottom")
pitch = b.circle(b.origin, 50, "pitch", construction=True)
b.diameter(pitch, 100, tol=0.3, at=(62, -92))  # the bolt circle locates the mating flange
dot = b.dot(35, 35, "bolt")
b.point_on(dot, pitch)
b.angle("u", b.line(b.origin, dot, "to_bolt", construction=True), 45)
p.hole(b, 14, next=True, id="bolt")
rep = p.build(OUT)
wall = rep.faces(where=lambda f: f["name"].startswith("bolt/") and not f["flat"])[0]
p.feature_pattern([wall], 4, about=axis("Z"), id="bolts")

# The second flange: the first with its holes, mirrored in the plane that halves the bend: of the two
# planes halfway between the plane of the pipe's start and that of its end, the one across the bend.
end_plane = p.plane("X", 100, id="end_plane")
rep = p.build(OUT)
flange_rim = rep.faces(where=lambda f: f["name"].startswith("flange/") and not f["flat"] and abs(f["middle"][2] + 6) < 1)[0]
holes = rep.faces(where=lambda f: f["name"].startswith(("bolt/", "bolts/")) and not f["flat"])
p.op("AddPlane", id="bisector", mode="Midplane", other=True, references=[axis("Z"), datum(end_plane)])
p.feature_mirror([flange_rim] + holes, datum("bisector"), id="flange2")

# The sheet: the elbow cut through its plane, the start flange face on under it, and a pictorial.
for helper in ("end_plane", "bisector", "flange2"):
    p.on_sheet(helper, on_drawing=False)
# (The tube's outside and inside: the faces the section's outer and inner circles swept.)
p.on_sheet("bend", at=(10, -42))
p.on_sheet("bolt/callout", count=4)
# Tolerances (a cast fitting, its flange faces and bolt holes finished): the faces seal against the
# mating flange (flat) and the bolt circle (± on its diameter, below).
rep = p.build(OUT)
face_a = "flange2/flange/end"
p.geometric_tolerance("Flatness", 0.1, on=topo(face_a), show_in="secA", at=(68, -108), to=(68, -88))
p.geometric_tolerance("Flatness", 0.1, on=topo("flange2/flange/end#2"), show_in="secA", at=(-110, 76), to=(-88.5, 76))
p.op("Section", id="A", along="Plane", plane=axis("Y"), offset=0.0)
p.drawing(views=[view("bottom", "Bottom", (170, 125)), section_view("secA", "A", (410, 270), of="bottom"),
                 view("iso", "IsometricFrontLeft", (165, 335))],
          scale=(1, 1), format="A2", id="drawing", general_tolerance="ISO 8062-3 DCTG 10",
          notes=[note("bottom", (-102, 60), "BOTH FLANGES"), note("bottom", (110, 50), "TUBE Ø60 × 5")])
rep = p.build(OUT, render=True, pictures=True, picture=os.path.join(HERE, "render.png"))
rep.show()

