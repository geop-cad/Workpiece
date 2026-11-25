import os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from shapecraft import Part, material, datum, view, projected, detail_view, topo, axis, note
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "5.shapecraft")
OD, ID, ND, W15 = 9, 10, 11, 21
material(here, "Plain carbon steel", 7800)
p = Part(material="Plain carbon steel")

# One grid bar: 1.5 wide, 12.5 left of the centre, long enough to run into the wall (a helper
# sketch, kept off the sheet), then 3 of them 12.5 apart and the set turned a quarter.
s = p.sketch("XY", id="bar_sk", on_drawing=False)
r = s.rect("b", -13.25, -30, 1.5, 60)
s.distance_u(r.points[0], r.points[1], 1.5, show_in="node", at=(-58, -17))
s.distance_v(r.points[1], r.points[2], 60)
s.point(-12.5, 0, "m0"); s.point_on("m0", "u"); s.distance_u("m0", s.origin, 12.5)
s.midpoint("m0", s.line(r.points[0], r.points[2], construction=True))
p.extrude(s, 6.5, combine="NewBody", id="bar")
p.feature_pattern(["bar/side/e12"], 3, along=axis("X"), spacing=12.5)
p.pattern("bar", 2, about=axis("Z"), angle=90, combine="Join")

# Trim the bars to a disc just inside the OD (helper), then the ring (OD 58, ID 52, 13 high) joins everything.
c = p.sketch("XY", id="trim_sk", on_drawing=False)
cc = c.circle(c.origin, 28); c.diameter(cc, 56)
p.extrude(c, 6.5, combine="Intersect", through=True, target=topo("bar"), id="trim")
q = p.sketch("XY", id="ring_sk", show_in="top")
od = q.circle(q.origin, 29); idc = q.circle(q.origin, 26)
q.diameter(od, 58, at=(56, 56)); q.diameter(idc, 52, at=(70, 17))
p.extrude(q, 13, id="ring")

# Round node, Ø4 on the bar crossings, 4 places about the axis.
n = p.sketch("XY", id="node_sk", show_in="top")
nc = n.point(-12.5, -12.5, "nc")
n.distance_u(n.origin, nc, 12.5, on_drawing=False); n.distance_v(n.origin, nc, 12.5, on_drawing=False)
nd = n.circle(nc, 2); n.diameter(nd, 4, show_in="node", at=(46, 20))
p.extrude(n, 6.5, id="node")
p.on_sheet("node", on_drawing=False)
r = p.build(out)
node_face = r.faces(near=(-14.5, -12.5, 3), where=lambda f: not f["flat"])
p.feature_pattern(node_face, 4, about=axis("Z"))

# Ridge: 90 degree V on the OD, tip at r = 30, full height; 8 about the axis.
g = p.sketch("XY", id="ridge_sk", show_in="ridge")
od2 = g.circle(g.origin, 29, construction=True); g.diameter(od2, 58, on_drawing=False)
g.point(0, 30, "A"); g.point(1.3, 28.7, "B1"); g.point(-1.3, 28.7, "B2")
g.point_on("A", "v"); g.distance(g.origin, "A", 30, on_drawing=False)
g.point_on("B1", od2); g.point_on("B2", od2)
l1 = g.line("B1", "A"); l2 = g.line("B2", "A"); lb = g.line("B1", "B2")
g.angle("v", l1, 45, at=(-14, 8)); g.angle(l2, "v", 45, at=(-14, -8))
p.extrude(g, 13, id="ridge")
p.on_sheet("ridge", on_drawing=False)
p.feature_pattern(["ridge/side/e10"], 8, about=axis("Z"))
r = p.build(out)
tips = r.edges(where=lambda e: e["straight"] and abs(e["length"] - 13) < 1e-6 and abs(abs(e["middle"][1]) - 30) < 1e-6 and abs(e["middle"][0]) < 1e-6)
# the sizes the sheet shows that no visible sketch carries: the tip to tip distance, the bar's half width, the pitches
p.dimension(*tips, show_in="top", at=(0, -64))
b_side = "bar/side/e13"; node_cyl = "node/side/e8#2"
sides = [r.faces(near=(x, 0, 3), where=lambda f: f["flat"] and abs(f["normal"][0]) > .99)[0] for x in (-13.25, -11.75)]
p.dimension(*sides, show_in="node", reference=True, at=(2, -38))
p.on_sheet("featurepattern1", on_drawing=False)
# the centre lines the original dimensions from: the grid's middle and the bar 12.5 off it, each way
for pid, ax, d in (("mid_x", "X", 0), ("cx", "X", -12.5), ("mid_y", "Y", 0), ("cy", "Y", -12.5)):
    p.plane(ax, d, id=pid); p.on_sheet(pid, on_drawing=False)
p.dimension(datum("mid_x"), datum("cx"), id="pitch_x", show_in="top", at=(-90, -2))
p.dimension(datum("mid_y"), datum("cy"), id="pitch_y", show_in="top", at=(-45, -47))
p.dimension(sides[0], datum("cx"), id="half_bar", show_in="node", at=(-36, 17))
p.dimension(sides[1], datum("cx"), id="half_ref", show_in="node", reference=True, at=(36, -17))
p.on_sheet("ring", show_in="front", at=(-58, 0))
p.on_sheet("bar", show_in="front", at=(46, -20))
p.drawing(scale=(1, 1), format="A3", comma=False, leading_zero=False, centre_lines=["all"] + [{"of": d, "view": "top"} for d in ("mid_x", "cx", "mid_y", "cy")],
    views=[view("top", "Top", (165, 185), style="HiddenDashed"),
    projected("front", "top", "Below", 87, style="HiddenDashed"),
    detail_view("node", "top", (-12.5, 12.5), 6, (85, 242), scale=(5, 1), letter="B"),
    detail_view("ridge", "top", (30, 0), 4, (270, 150), scale=(5, 1), letter="A"),
    view("iso", "Isometric", (330, 235)), view("iso2", "IsometricBelowBackLeft", (360, 120))],
    notes=[note(None, (0, 0), "(4) PLACES", dimension="node_sk/d%d" % ND), note("node", (58, 33), "(CENTERPOINT ON CENTERLINES)"),
           note(None, (0, 0), "OD", dimension="ring_sk/d%d" % OD), note(None, (0, 0), "ID", dimension="ring_sk/d%d" % ID),
           note(None, (0, 0), "TYP", dimension="half_bar"),
           note(None, (0, 0), "CENTERLINE TO CENTERLINE", dimension="pitch_x"), note(None, (0, 0), "CENTERLINE TO CENTERLINE", dimension="pitch_y"),
           note(None, (0, 0), "TYP", dimension="bar_sk/d%d" % W15),
           note("ridge", (0, -33), "(8) PLACES EVENLY SPACED"),
           note("iso", (10, -42), "THIS SHAPE PATTERNED (8) PLACES EVENLY SPACED", to=(30, 0, 6.5)),
           note("iso", (-30, 38), "1.5 WIDE TYP", to=(0, -10, 6.5))])
r = p.build(out, render=True); r.show(); print(r.volume, r.volume*0.0078)
