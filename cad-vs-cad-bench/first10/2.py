import os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import datum, Part, view, projected, section_view, topo, axis, note, material
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "2.workpiece")
material(here, "Plain carbon steel", 7800)
p = Part(material="Plain carbon steel")
s = p.sketch("YZ", id="profile")
for n, y, z in [("A",-105,0),("B",-60,0),("C",60,0),("D",105,0),("E",105,15),("F",68.2,15),("K",-68.2,15),("L",-105,15),("Ti",0,35),("T",0,50)]:
    s.point(y, z, n)
s.point(0, -33.9, "c")
ai = s.arc("c", "C", "B"); ao = s.arc("c", "F", "K")
la = [s.line("A","B"), s.line("C","D"), s.line("E","F"), s.line("K","L")]
lv = [s.line("D","E"), s.line("L","A")]
lad = s.line("A","D"); s.construction(lad)
for l in (la[0], la[2], la[3]): s.horizontal(l)
for l in lv: s.vertical(l)
s.point_on("c", "v"); s.point_on("Ti", "v"); s.point_on("Ti", ai)
s.point_on("B", "u"); s.horizontal("B", "C"); s.distance_u("B", "C", 120, at=(0, -36))
s.distance_u("A", "D", 210, at=(0, -44)); s.midpoint(s.origin, lad)
s.equal(lv[0], lv[1])
s.point_on("T", ao); s.point_on("T", "v")
s.distance_v(s.origin, "T", 50, at=(-76, 8), id="tangent"); s.distance_v("Ti", "T", 15, at=(33, 17))
# the flange is as thick as the arch
rib = s.line("Ti", "T", construction=True); s.equal(lv[0], rib)
p.extrude(s, 80, combine="NewBody", symmetric=True, id="arch")
r = p.build(out)
# the channel opens upward: the two flange tops and the outer arc
tops = r.faces(where=lambda f: (f["flat"] and f["normal"][2] > .99 and abs(f["middle"][2] - 15) < 1e-6) or
               (not f["flat"] and abs(f["middle"][0]) < 1e-6 and abs(f["middle"][2] - 50) < 1e-6))
p.shell(tops, 5, id="hollow")
p.on_sheet("hollow", on_drawing=False)
r = p.build(out)
floor = r.faces(near=(0, 0, 40), where=lambda f: not f["flat"])[0]

# Boss tube: Ø38 on the 58 level down to the channel floor; its centre dot drills the Ø28 through the floor.
top = p.plane("Z", 58, id="top58")
b = p.sketch(datum(top), id="boss_sketch", show_in="top")
bo = b.circle(b.origin, 19); b.diameter(bo, 38, at=(40, 24))
bd = b.dot(0, 0); b.coincident(bd, b.origin)
p.op("Extrude", "boss", sketch={"Profile": b.id}, extent="ToFace", up_to=topo(floor), flip=True)
p.hole(b, 28, id="bore")
p.on_sheet("bore/callout", show_in="top", at=(40, 40))
r = p.build(out)
p.on_sheet("top58", show_in="front", at=(-38, 12))

# Corner pads: the 210 x 80 outline with the 45 degree octagon pocket inside it (5 wall).
q = p.sketch("XY", id="pads_sketch", show_in="top")
R = q.rect("r", -40, -105, 80, 210)
dg = q.line(R.points[0], R.points[2], construction=True); q.midpoint(q.origin, dg)
q.distance_u(R.points[0], R.points[1], 80, on_drawing=False); q.distance_v(R.points[1], R.points[2], 210, at=(0, -50))
O = q.polyline([(11,100),(-11,100),(-35,76),(-35,-76),(-11,-100),(11,-100),(35,-76),(35,76)], True, "o")
P = O.points; L = O.curves
for a_, b_ in ((0,1),(2,7),(3,6),(4,5)): q.symmetric(P[a_], P[b_], "v")
for a_, b_ in ((0,5),(2,3)): q.symmetric(P[a_], P[b_], "u")
q.distance(P[4], P[5], 22, at=(-70, -9))
q.angle(L[3], L[4], 45, at=(-60, 27))
q.distance_v(P[0], R.points[2], 5, on_drawing=False)
q.distance_v(P[0], R.points[2], 5, reference=True, at=(64, -22))
q.distance_u(P[7], R.points[2], 5, at=(-30, -24), id="wall", on_drawing=False)
p.extrude(q, 15, combine="NewBody", id="pads")
p.on_sheet("pads", on_drawing=False)
# the ring also fills the void under the arch: trim it to the arch's own profile, then join
p.extrude(s, 80, combine="Intersect", through=True, symmetric=True, target=topo("pads"), id="trim")
p.op("CombineBodies", "pads_in", how="Join", target=topo("arch"), tool=topo("pads"))
r = p.build(out)

# the flange thickness, read off the left end of the arch
p.dimension(r.edges(where=lambda e: e["straight"] and abs(e["length"] - 80) < 1e-6 and abs(e["middle"][1] + 105) < 1e-6 and abs(e["middle"][2]) < 1e-6)[0],
            r.edges(where=lambda e: e["straight"] and abs(e["length"] - 80) < 1e-6 and abs(e["middle"][1] + 105) < 1e-6 and abs(e["middle"][2] - 15) < 1e-6)[0],
            id="flange", show_in="front", at=(-62, -9))

# Four Ø7 holes, 14 x 90 degree countersunk, on the 190 x 60 rectangle.
h = p.sketch(topo(r.faces(near=(0, 84, 15), where=lambda f: f["flat"] and f["normal"][2] > .99)[0]), id="holes_sketch", show_in="top")
H = h.rect("h", -30, -95, 60, 190)
dg2 = h.line(H.points[0], H.points[2], construction=True); h.midpoint(h.origin, dg2)
h.distance_u(H.points[0], H.points[1], 60, at=(-80, 1)); h.distance_v(H.points[1], H.points[2], 190, at=(0, -42))
for pn in H.points:
    d = h.dot(*h._points[pn]); h.coincident(d, pn)
for l in H.curves: h.construction(l)
p.hole(h, 7, kind="Countersink", head=14, head_angle=90, id="cs")
p.on_sheet("cs/callout", show_in="top", at=(-22, 38))
p.dimension("arch/edge[cs/h2/side/e2+side/e20]", kind="Diameter", reference=True, show_in="front", at=(62, -26))
p.dimension("cs/h2/ring/v1", kind="Diameter", reference=True, show_in="front", at=(62, 18))
p.dimension("thick/f3/far", "arch/side/e23", reference=True, show_in="secview", at=(40, -22))
p.dimension("boss/side/e8", "bore/side/e1", reference=True, show_in="secview", at=(-14, 14))
p.dimension("arch/side/e19", "thick/f0/far", reference=True, show_in="secview", at=(-62, -9))
p.dimension("arch/side/e26", "thick/f1/far", reference=True, show_in="secview", at=(-40, 8))
sec = p.op("Section", "secA", plane=axis("X"))
p.drawing(scale=(1, 2), centre_lines=["all", {"of": "origin/z", "view": "front", "label": "CL SYM"},], views=[view("front", "Front", (106, 126), style="HiddenDashed"), projected("top", "front", "Above", 98, style="HiddenDashed"),
    section_view("secview", "secA", (252, 126), of="top"), view("iso", "Isometric", (240, 248)), view("iso2", "IsometricBelowBackLeft", (352, 248))],
    notes=[note(None, (0, 0), "TO TANGENT OF ARC", dimension="profile/tangent"), note(None, (0, 0), "HOLE THRU", dimension="bore/callout"), note("top", (-16, -29), "5 WALL TYP", to=(37.5, -30, 15))])
r = p.build(out, render=True); r.show(); print(r.volume, r.volume*0.0078)
