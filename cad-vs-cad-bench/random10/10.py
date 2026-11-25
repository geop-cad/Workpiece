import math, os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import datum, profile, Part, axis, topo, material, view, projected, note
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "10.workpiece")
material(here, "PLAIN CARBON STEEL", 7800)
p = Part(unit="Millimetre", material="PLAIN CARBON STEEL")

# Tab outline (u = y, v = z): 60 wide, 10 straight, tangent sides up to the R38 arc about the hole centre 60 up.
s = p.sketch("YZ", show_in="left")
A = s.point(-30, 0, "A"); B = s.point(30, 0, "B"); C = s.point(30, 10, "C"); F = s.point(-30, 10, "F")
D = s.point(37, 54, "D"); E = s.point(-37, 54, "E"); O = s.point(0, 60, "O")
lab = s.line(A, B); lbc = s.line(B, C); lcd = s.line(C, D); lef = s.line(E, F); lfa = s.line(F, A)
arc = s.arc(O, D, E)
s.horizontal(lab); s.vertical(lbc); s.vertical(lfa)
s.midpoint(s.origin, lab)
s.distance_u(A, B, 60, show_in="top"); s.distance_v(B, C, 10)
s.horizontal(C, F)
s.point_on(O, "v"); s.distance_v(s.origin, O, 60)
s.radius(arc, 38)
s.tangent(lcd, arc); s.tangent(lef, arc)
p.extrude(s, 90, combine="NewBody", symmetric=True, id="blank")

# Open the channel: a pocket from the inside floor (z = 5) up through everything, leaving the 5 walls.
rep = p.build(out)
outer = rep.edges(where=lambda e: e["straight"] and abs(e["from"][2]) < 1e-6 and abs(e["to"][2]) < 1e-6 and abs(abs(e["middle"][0]) - 45) < 1e-6)
floor = p.plane("Z", 5, id="floor")
g = p.sketch(datum(floor), id="channel_sk", show_in="front")
g.project("wl", outer[0]); g.project("wr", outer[1])
r = g.rect("r", -40, -40, 80, 80)
g.symmetric(r.points[0], r.points[3], "u")
g.distance_u("wl_a", r.points[0], 5, at=(-30, 40))
g.distance_u(r.points[1], "wr_a", 5, at=(30, 40))
g.equal(r.curves[0], r.curves[1])
p.cut(g, through=True, id="channel")
rep = p.build(out)
inner = rep.edges(where=lambda e: e["straight"] and abs(e["from"][2] - 5) < 1e-6 and abs(e["to"][2] - 5) < 1e-6 and abs(abs(e["middle"][0]) - 40) < 1e-6)
print(len(outer), len(inner))
p.fillet(outer, 8, id="outer_bend")
p.on_sheet("outer_bend", show_in="front", at=(36, -42))
p.fillet(inner, 3, id="inner_bend")
p.on_sheet("inner_bend", show_in="front", at=(-16, -5))
p.on_sheet("floor", on_drawing=False)

# Eyes: D30 through the left wall, D18 through the right wall, same height (the 60 is the one the blank sketch already gives).
rep = p.build(out)
face = {x: rep.faces(where=lambda f, x=x: f["flat"] and f["normal"][0] * x > 0.99 and f["middle"][0] * x > 40)[0] for x in (1, -1)}
print(face)
for name, x, dia in (("left", 1, 30), ("right", -1, 18)):
    h = p.sketch(topo(face[x]), id=name + "_eye", on_drawing=False)
    d = h.dot(0, 60)
    h.point_on(d, "v")
    h.distance_v(h.origin, d, 60)
    p.op("Hole", id=name + "_hole", sketch=profile(h), diameter=float(dia), kind="Simple", extent="ToNext", flip=(x < 0))
    p.on_sheet(name + "_hole", show_in=name, at=(-40, 38) if x > 0 else (40, 38))
rep = p.build(out)
for n in ("left_eye", "right_eye"): print(n, rep.sketch(n)["points"])
# Flat top on the right tab at 92: a square cut across it, only through that wall.
f = p.sketch(topo(face[-1]), id="flat", show_in="right")
fr = f.rect("f", -40, 92, 80, 80)
f.symmetric(fr.points[0], fr.points[1], "v")
f.distance_v(f.origin, fr.points[0], 92)
f.distance_u(fr.points[0], fr.points[1], 80, on_drawing=False)
f.equal(fr.curves[0], fr.curves[1])
p.op("Extrude", id="flat_top", sketch=profile(f), combine="Cut", extent="ToNext")
# The 5 mm walls and floor, between their outer and inner faces.
rep = p.build(out)
floor_in = rep.faces(where=lambda f: f["flat"] and f["normal"][2] > 0.99 and abs(f["middle"][2] - 5) < 1e-6)[0]
floor_out = rep.faces(where=lambda f: f["flat"] and f["normal"][2] < -0.99 and abs(f["middle"][2]) < 1e-6)[0]
p.dimension(floor_out, floor_in, show_in="front", id="floor_t", at=(-26, -30))
p.dimension("blank/side/e15", "blank/side/e18", show_in="right", id="w_right")
p.dimension("blank/side/e15", "blank/side/e18", show_in="left", reference=True, id="w_left")
p.drawing(scale=(1, 2), views=[view("front", "Right", (205, 105)), projected("top", "front", "Above", 75), projected("left", "front", "Left", 75), projected("right", "front", "Right", 75, style="HiddenDashed"), view("iso", "Isometric", (350, 228)), view("iso2", "IsometricBackLeft", (372, 118))],
  notes=[note(None, (0, 0), "(2) PLACES", dimension="inner_bend"), note(None, (0, 0), "(2) PLACES", dimension="outer_bend"),
         note(None, (0, 0), "HOLE", dimension="left_hole"), note(None, (0, 0), "HOLE", dimension="right_hole"),
         note("left", (-62, -46), "NOT TANGENT", to=(-45, -30, 10)),
         note(None, (40, 270), "NOTE:"), note(None, (72, 264), "1. MAY BE MODELED AS SHEET METAL"), note(None, (72, 258), "OR AS STANDARD MODEL")])
rep = p.build(out, render=True); rep.show()
print("mass g", rep.volume*7.8e-3)
