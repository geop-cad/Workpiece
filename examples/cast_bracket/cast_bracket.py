"""Cast angle bracket: an L profile extruded, a drafted wall, a rib, mirrored slots, organic fillets.

The L is one sketch extruded both ways from the middle plane; the wall's inner face gets a casting
draft; a rib in the middle plane stiffens the corner; one mounting slot is cut and mirrored; the
wall takes a Ø20 H7 bore; fillets round every inside corner so the casting flows.
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, profile, topo, view, projected, note, datum

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "cast_bracket.wpc")
p = Part(material="Aluminium", title="CAST ANGLE BRACKET", file="cast_bracket.wpc")

# The L, on ZX (u = z, v = x): a base 70 long and 12 thick, a wall 10 thick and 80 high.
s = p.sketch("ZX", id="l_profile", show_in="right")
A, B, C = s.point(0, -35, "A"), s.point(0, 35, "B"), s.point(12, 35, "C")
D, E, F = s.point(12, -25, "D"), s.point(80, -25, "E"), s.point(80, -35, "F")
for a, b in ((A, B), (B, C), (C, D), (D, E), (E, F), (F, A)):
    s.line(a, b)
s.vertical(A, B); s.horizontal(B, C); s.vertical(C, D); s.horizontal(D, E); s.vertical(E, F); s.horizontal(F, A)
s.point_on(A, "v")
s.symmetric(A, B, "u")
s.distance_v(A, B, 70)
s.distance_u(B, C, 12)
s.distance_v(F, E, 10, at=(30, 46))
s.distance_u(A, F, 80)
p.extrude(s, 100, symmetric=True, combine="NewBody", id="bracket")
rep = p.build(OUT)

# A 2° casting draft on the wall's inner face, about the wall's top: the wall thickens towards the base.
inner = rep.faces(where=lambda f: f["flat"] and f["normal"][0] > 0.99 and f["middle"][2] > 20)[0]
wall_top = rep.faces(where=lambda f: f["flat"] and f["normal"][2] > 0.99 and abs(f["middle"][2] - 80) < 1e-6)[0]
p.op("Draft", id="draft", faces=[topo(inner)], neutral=topo(wall_top), angle=2.0)
rep = p.build(OUT)

# A rib 40 off the middle plane: its free edge from the wall to the base, drawn against both (their
# outlines projected from the end face), 8 thick; mirrored to the other side.
on_end = lambda e: e["straight"] and abs(e["from"][1] - 50) < 1e-6 and abs(e["to"][1] - 50) < 1e-6
wall_edge = rep.edges(where=lambda e: on_end(e) and e["middle"][0] > -30 and abs(e["from"][2] - e["to"][2]) > 50)[0]
back_edge = rep.edges(where=lambda e: on_end(e) and e["middle"][0] < -34 and abs(e["from"][2] - e["to"][2]) > 50)[0]
base_edge = rep.edges(where=lambda e: on_end(e) and abs(e["from"][2] - 12) < 1e-6 and abs(e["to"][2] - 12) < 1e-6)[0]
rib_plane = p.plane("Y", 40, id="rib_plane")
r = p.sketch(datum(rib_plane), id="rib_edge", show_in="right")
r.project("wall", wall_edge)
r.project("base", base_edge)
r.project("back", back_edge)
r0, r1 = r.point(62, -24, "r0"), r.point(12, 23, "r1")
r.line(r0, r1, "edge")
r.point_on(r0, "wall"); r.point_on(r1, "base")
r.distance_u(r1, r0, 50)
r.point_line(r1, "back", 58)
p.op("Rib", id="rib", sketch=profile(r), thickness=8.0)
rep = p.build(OUT)
rib_side = rep.faces(where=lambda f: f["name"].startswith("rib") and f["flat"] and abs(f["normal"][1]) > 0.99)[0]
p.feature_mirror([rib_side], "Y", id="rib_mirror")

# A mounting slot in the base, 9 wide, and its mirror image.
rep = p.build(OUT)
top = rep.faces(where=lambda f: f["flat"] and f["normal"][2] > 0.99 and abs(f["middle"][2] - 12) < 1e-6)
t = p.sketch(topo(top[0]), id="slot_outline", show_in="top")
sl = t.slot("s", 10, 22, 14, 9)
c0, c1 = sl.points[0], sl.points[1]
t.horizontal(c0, c1)
t.distance_u(t.origin, c0, 10)
t.distance_v(t.origin, c0, 22)
p.cut(t, through=True, id="slot")
rep = p.build(OUT)
slot_wall = rep.faces(where=lambda f: f["name"].startswith("slot/") and not f["flat"])[0]
p.feature_mirror([slot_wall], "Y", id="slot_mirror")

# The bore for a shaft, Ø20 H7, drilled from the wall's outside.
rep = p.build(OUT)
outside = rep.faces(where=lambda f: f["flat"] and f["normal"][0] < -0.99)[0]
b = p.sketch(topo(outside), id="bore_place", show_in="front")
bd = b.dot(0, 55, "bore")
b.point_on(bd, "v")
b.distance_v(b.origin, bd, 55)
p.hole(b, 20, id="bore", **{"class": "H7"})

# Fillets: R6 in the corner between wall and base, R3 where the ribs meet them, R2 on the ribs' edges.
rep = p.build(OUT)
corner = rep.edges(where=lambda e: e["straight"] and abs(e["middle"][2] - 12) < 1e-6 and -30 < e["middle"][0] < -20)
p.fillet(corner, 6, id="corner")
rep = p.build(OUT)
joints = rep.edges(where=lambda e: e["name"].startswith("bracket/edge[") and "rib" in e["name"])
p.fillet(joints, 3, id="rib_roots")
rep = p.build(OUT)
crests = rep.edges(where=lambda e: e["name"].startswith(("rib/", "rib_mirror/")) and "/along/" in e["name"])
# (Only the crests: their tangent chain runs on into the edges where the ribs' sides meet the
# bracket, which the R3 roots have rounded already.)
p.fillet(crests, 2, id="rib_crests", tangent=False)
rep = p.build(OUT)
ends = rep.edges(where=lambda e: e["straight"] and abs(e["middle"][0] - 35) < 1e-6 and abs(abs(e["middle"][1]) - 50) < 1e-6)
p.fillet(ends, 10, id="base_corners")

# The sheet: the inside of the wall from the front, the plan above it, the L profile to the right
# (where the draft and the rib show), and a pictorial; the small rounds said once in a note.
p.on_sheet("rib_roots", on_drawing=False)
p.on_sheet("rib_crests", on_drawing=False)
p.on_sheet("base_corners", at=(-50, -42))
p.on_sheet("draft", at=(10, 56))
p.drawing(views=[view("front", "Front", (85, 120)), projected("top", "front", "Above", 115),
                 projected("right", "front", "Right", 120), view("iso", "Isometric", (330, 205))],
          scale=(1, 1), format="A3", id="drawing",
          notes=[note("right", (95, -45), "UNSPECIFIED FILLETS R3, RIB EDGES R2")])
rep = p.build(OUT, render=True, pictures=True, picture=os.path.join(HERE, "render.png"))
rep.show()
