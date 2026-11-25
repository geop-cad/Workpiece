import os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import datum, profile, Part, axis, topo, material, view, projected, note
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "9.workpiece")
material(here, "PLAIN CARBON STEEL", 7800)
p = Part(unit="Millimetre", material="PLAIN CARBON STEEL")

# Base block 100 x 60 x 30, then drafted 30 deg (ends) and 15 deg (sides) from its bottom.
s = p.sketch("XY", show_in="top")
r = s.rect("r", -50, -30, 100, 60)
dg = s.line(r.points[0], r.points[2], construction=True); s.midpoint(s.origin, dg)
s.distance_u(r.points[0], r.points[1], 100)
s.distance_v(r.points[1], r.points[2], 60)
p.extrude(s, 30, combine="NewBody", id="base")
rep = p.build(out)
bottom = rep.faces(where=lambda f: f["flat"] and f["normal"][2] < -0.99)[0]
ends = rep.faces(where=lambda f: f["flat"] and abs(f["normal"][0]) > 0.99)
sides = rep.faces(where=lambda f: f["flat"] and abs(f["normal"][1]) > 0.99)
p.op("Draft", id="draft_ends", faces=[topo(f) for f in ends], neutral=topo(bottom), angle=30.0, flip=True)
p.op("Draft", id="draft_sides", faces=[topo(f) for f in sides], neutral=topo(bottom), angle=15.0, flip=True)
p.on_sheet("draft_ends", show_in="front")
p.on_sheet("draft_sides", show_in="side", at=(13, -27))

# Ring D110 x 36 wide, axis along y, centre 140 up, shelled open at both ends to the 10 wall.
g = p.sketch("ZX", show_in="front")
cen = g.point(140, 0, "cen")
o = g.circle(cen, 55)
g.diameter(o, 110, at=(-52, 40))
g.point_on(cen, "u"); g.distance_u(g.origin, cen, 140, at=(-52, -10))
p.extrude(g, 36, combine="Join", symmetric=True, id="ring")
p.on_sheet("ring", show_in="side")
rep = p.build(out)
ends = rep.faces(where=lambda f: f["flat"] and abs(f["normal"][1]) > 0.99 and f["middle"][2] > 100)
p.shell(ends, 10, id="wall")
rep = p.build(out)

# Column D24 from the base top up to the ring.
basetop = rep.faces(where=lambda f: f["flat"] and f["normal"][2] > 0.99 and abs(f["middle"][2] - 30) < 1e-6)[0]
c = p.sketch(topo(basetop))
cc = c.circle(c.origin, 12); c.diameter(cc, 24, show_in="front", at=(34, -19))
outer = rep.faces(where=lambda f: not f["flat"] and f["area"] > 12000)
p.op("Extrude", id="column", sketch=profile(c), combine="Join", extent="ToFace", up_to=topo(outer[0]))

# Counterbore floor at 90 from the underside: the D12 hole goes down from it, the D22 bore up into the ring.
floor = p.plane("Z", 90, id="cbplane")
rt = p.plane("Z", 140, id="ringmid")
h = p.sketch(datum(rt), on_drawing=False)
d = h.dot(0, 0); h.coincident(d, h.origin)
p.hole(h, 12, kind="Counterbore", head=22, head_up_to=datum("cbplane"), id="bore")
p.on_sheet("bore", show_in="side", at=(33, -16))

# Top wall: D26 hole and an 8 wide slot across the ring, from the ring's centre plane up.
sl = p.sketch(datum(rt), show_in="top")
sr = sl.rect("sl", -4, -20, 8, 40)
sdg = sl.line(sr.points[0], sr.points[2], construction=True); sl.midpoint(sl.origin, sdg)
sl.distance_u(sr.points[0], sr.points[1], 8, at=(18, 24))
sl.distance_v(sr.points[1], sr.points[2], 40, on_drawing=False)
t = p.sketch(datum(rt), show_in="top")
td = t.dot(0, 0); t.coincident(td, t.origin)
p.op("Hole", id="topbore", place="Sketch", sketch=profile(t), diameter=26.0, kind="Simple", extent="ToNext", flip=True)
p.on_sheet("topbore", show_in="top")
p.op("Extrude", id="slot", sketch=profile(sl), combine="Cut", extent="ToNext")
p.on_sheet("cbplane", on_drawing=False); p.on_sheet("ringmid", on_drawing=False)
rep = p.build(out)
for st in ['column','bore','topbore','slot']: print(st, rep.volumes_after(st))
# 90 from the base's underside up to the counterbore floor.
floor = rep.faces(where=lambda f: f["flat"] and f["normal"][2] > 0.99 and abs(f["middle"][2] - 90) < 1e-6)
foot = rep.faces(where=lambda f: f["flat"] and f["normal"][2] < -0.99)
p.dimension(foot[0], floor[0], show_in="front", id="cbore_depth", at=(78, -26))
p.on_sheet("wall", show_in="front", at=(40, 36))


p.drawing(scale=(1, 2), views=[view("front", "Right", (100, 90), style="HiddenDashed"), projected("top", "front", "Above", 120),
          projected("side", "front", "Right", 110, style="HiddenDashed"), view("iso", "Isometric", (285, 200)), view("iso2", "IsometricBelowBackLeft", (368, 190))],
  notes=[note(None, (0, 0), "THRU", dimension="bore/callout"),
         note("iso", (22, -64), "15\u00b0", to=(0, 34, 15)), note("iso", (-30, -60), "30\u00b0", to=(58.7, 0, 15)),
         note(None, (0, 0), "WALL", dimension="wall"), note(None, (0, 0), "TO C'BORE", dimension="cbore_depth")])
rep = p.build(out, render=True); rep.show()
print("mass g", rep.volume*7.8e-3)
