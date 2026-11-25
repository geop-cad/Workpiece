"""Hydraulic manifold block: drilled ports that meet inside an aluminium block.

A pressure port drilled down from the top with a spot face, a side port with a counterbore and an
end port with a countersink that run into it, four tapped M6 holes for a valve (one hole, mirrored
twice), and the outer top edges chamfered. A section through the ports shows them meet.
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, axis, note, view, section_view, topo

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "manifold_block.workpiece")
p = Part(material="Aluminium", title="MANIFOLD BLOCK", file="manifold_block.workpiece")

# The block: 90 long (y), 60 deep (x), 50 high, standing on the origin.
b = p.sketch("XY", id="block_outline", show_in="top")
r = b.rect("b", -30, -45, 60, 90)
b.symmetric(r.points[0], r.points[1], "v")
b.symmetric(r.points[1], r.points[2], "u")
b.distance_u(r.points[0], r.points[1], 60)
b.distance_v(r.points[1], r.points[2], 90)
p.extrude(b, 50, combine="NewBody", id="block")

# The pressure port P from the top: Ø10, 32 deep, with a Ø18 spot face.
t = p.sketch(topo("block/end"), id="port_p", show_in="top")
t.coincident(t.dot(0, 0, "p"), t.origin)
p.hole(t, 10, depth=32, kind="Counterbore", head=18, head_depth=1, point=180, id="P")

# The side port A into the front face (x = 30): Ø8 with a Ø15 counterbore, 30 deep to P's axis.
rep = p.build(OUT)
front = rep.faces(where=lambda f: f["flat"] and f["normal"][0] > 0.99)[0]
a = p.sketch(topo(front), id="port_a", show_in="front")
ad = a.dot(0, 25, "a")
a.point_on(ad, "v")
a.distance_v(a.origin, ad, 25)
p.hole(a, 8, depth=30, kind="Counterbore", head=15, head_depth=6, point=180, id="A")

# The end port B into the right end (y = 45): Ø8, countersunk Ø14 × 90°, 45 deep to P's axis.
right = rep.faces(where=lambda f: f["flat"] and f["normal"][1] > 0.99)[0]
e = p.sketch(topo(right), id="port_b", show_in="secB")
bd = e.dot(0, 18, "b")
e.point_on(bd, "v")
e.distance_v(e.origin, bd, 18)
p.hole(e, 8, depth=45, kind="Countersink", head=14, head_angle=90, point=180, id="B")

# The valve's four tapped M6 holes, 12 deep: one hole step at four dots placed symmetrically.
v = p.sketch(topo("block/end"), id="valve_holes", show_in="top")
d1, d2, d3, d4 = v.dot(16, 30, "v1"), v.dot(-16, 30, "v2"), v.dot(-16, -30, "v3"), v.dot(16, -30, "v4")
v.symmetric(d1, d2, "v"); v.symmetric(d2, d3, "u"); v.symmetric(d3, d4, "v")
v.distance_u(v.origin, d1, 16)
v.distance_v(v.origin, d1, 30)
p.hole(v, 6, depth=12, kind="Tapped", size="M6", id="M6")

# The top's outer edges chamfered.
rep = p.build(OUT)
top = rep.edges(where=lambda e: e["straight"] and abs(e["from"][2] - 50) < 1e-6 and abs(e["to"][2] - 50) < 1e-6
                and (abs(abs(e["middle"][0]) - 30) < 1e-6 or abs(abs(e["middle"][1]) - 45) < 1e-6))
p.chamfer(top, 1, id="top_chamfer")


# The sheet: the plan with two cutting lines, the section through P and B in line under it, the section
# through P and A beside that, an outside view of the front face with port A, and a pictorial.
p.on_sheet("top_chamfer", on_drawing=False)
p.on_sheet("A/callout", show_in="secA")
p.on_sheet("P/callout", at=(-20, 45))
p.on_sheet("B/callout", show_in="secB")
p.op("Section", id="cut_b", along="Plane", plane=axis("X"), offset=0.0)
p.op("Section", id="cut_a", along="Plane", plane=axis("Y"), offset=0.0)
p.drawing(views=[view("top", "Top", (100, 220)),
                 section_view("secB", "cut_b", (100, 110), of="top", letter="B"),
                 section_view("secA", "cut_a", (215, 110), of="top", letter="A"),
                 view("front", "Front", (215, 220)),
                 view("iso", "Isometric", (335, 200))], scale=(1, 1), format="A3", id="drawing",
          # (The valve holes' centre lines only where the holes are drawn: in the plan.)
          centre_lines=["all"] + [{"of": "M6", "view": v, "hide": True} for v in ("front", "secA", "secB")],
          notes=[note("top", (58, 40), "1×45°", to=(-29.5, 45, 50))])
rep = p.build(OUT, render=True, pictures=True, picture=os.path.join(HERE, "render.png"))
rep.show()
