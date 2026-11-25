"""A flange: a dimensioned disc with a bore, one bolt hole patterned six times, a rounded rim."""
import math, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "python"))  # a checkout; `pip install workpiece` makes this line unnecessary
from workpiece import Part, profile, axis

out = sys.argv[1] if len(sys.argv) > 1 else "flange.workpiece"
p = Part(material="Steel")

# The disc with its bore: two circles on the origin, each with its diameter.
s = p.sketch("XY", show_in="top")
outer = s.circle(s.origin, 45)
bore = s.circle(s.origin, 12)
s.diameter(outer, 100)
s.diameter(bore, 30)
disc = p.extrude(s, 12, combine="NewBody")

# One bolt hole on the pitch circle, placed by its dimensions.
h = p.sketch("XY", show_in="top")
c = h.point(36, 0.5)
h.horizontal(h.origin, c)
h.distance(h.origin, c, 37.5)
hole = h.circle(c, 4)
h.diameter(hole, 9)
first = p.cut(h, through=True)

report = p.build(out)
# ...patterned six times about z, by the hole's own wall.
wall = report.faces(near=(37.5, 0, 6), where=lambda f: not f["flat"])
p.op("Pattern", along="Circle", count=6, angle=360.0, axis=axis("Z"),
     features=[{"Topology": wall[0]}])
report = p.build(out)
# The outer top rim, rounded.
rim = report.edges(where=lambda e: not e["straight"] and abs(e["middle"][2] - 12) < 1e-6
                   and abs(math.hypot(*e["middle"][:2]) - 50) < 1e-6)
p.fillet(rim, 2)
# A sheet with the usual views and every dimension of the sketches on it.
p.drawing(scale=(1, 2))
report = p.build(out, render=True)
report.show()
print("volume", report.volume)
