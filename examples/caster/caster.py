"""Swivel-plate caster: a shelled steel fork, a two-material wheel (an aluminium hub and a rubber tyre:
one part, two bodies, each weighing by its own material) turning on an axle pin.

The fork is a block rounded on top and shelled open at the bottom and both ends, which leaves a top
plate and two legs; the axle hole is cut through both legs at once. The wheel is two revolved bodies;
an Appearance step makes the tyre rubber. In the assembly the pin is fastened in the fork and the wheel
turns on it (a revolute joint), centred between the legs.
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, anchor, axis, topo, view, projected, section_view

HERE = os.path.dirname(os.path.abspath(__file__))
at = lambda name: os.path.join(HERE, name)

# --- The fork: a 60 × 50 × 70 block, its two top edges along x rounded R10, shelled 3 thick open at the
# bottom and at both ends; a Ø8 axle hole 20 above the bottom through both legs; a Ø10 king pin hole.
fork = Part(material="Steel", title="FORK", file="fork.wpc")
s = fork.sketch("XY", id="fork_plan", show_in="top")
r = s.rect("f", -30, -25, 60, 50)
s.symmetric(r.points[0], r.points[1], "v")
s.symmetric(r.points[1], r.points[2], "u")
s.distance_u(r.points[0], r.points[1], 60)
s.distance_v(r.points[1], r.points[2], 50)
fork.extrude(s, 70, combine="NewBody", id="block")
rep = fork.build(at("fork.wpc"))
fork.fillet(rep.edges(where=lambda e: e["straight"] and abs(e["middle"][2] - 70) < 1e-6 and abs(e["from"][1] - e["to"][1]) < 1e-6), 10,
            id="crown")
rep = fork.build(at("fork.wpc"))
fork.shell(rep.faces(where=lambda f: f["flat"] and (f["normal"][2] < -0.99 or abs(f["normal"][0]) > 0.99)), 3, id="wall")
a = fork.sketch("ZX", id="axle_hole", show_in="front")
c = a.point(20, 0, "c")
a.point_on(c, "u")
a.distance_u(a.origin, c, 20)
a.diameter(a.circle(c, 4, "hole"), 8)
fork.cut(a, through=True, symmetric=True, id="axle")
rep = fork.build(at("fork.wpc"))
top = rep.faces(where=lambda f: f["flat"] and f["normal"][2] > 0.99)[0]
k = fork.sketch(topo(top), id="king_pin", show_in="top")
k.coincident(k.dot(0, 0, "k"), k.origin)
fork.hole(k, 10, next=True, id="king")
fork.drawing(views=[view("front", "Front", (90, 130)), projected("top", "front", "Above", 95),
                    projected("right", "front", "Right", 110), view("iso", "Isometric", (330, 210))],
             scale=(1, 1), format="A3", id="drawing")

# --- The wheel: Ø75 × 30, its axis along y. The hub (Ø40, a Ø8 bore) and the tyre (Ø40 to Ø75, its
# outer edges rounded R6) are two bodies; the tyre is rubber.
wheel = Part(material="Aluminium", title="WHEEL", file="wheel.wpc")
h = wheel.sketch("YZ", id="hub_profile", show_in="secW")
ax = h.rotation_axis(h.origin, (1, 0))
h.horizontal(ax)
p0, p1, p2, p3 = h.point(-15, 4, "a"), h.point(15, 4, "b"), h.point(15, 20, "c"), h.point(-15, 20, "d")
for u, v in ((p0, p1), (p1, p2), (p2, p3), (p3, p0)):
    h.line(u, v)
h.horizontal(p0, p1); h.vertical(p1, p2); h.horizontal(p2, p3); h.vertical(p3, p0)
h.symmetric(p0, p1, "v")
h.distance_u(p0, p1, 30)
h.diameter_about(p0, ax, 8)
h.diameter_about(p3, ax, 40)
wheel.revolve(h, combine="NewBody", id="hub")
t = wheel.sketch("YZ", id="tyre_profile", show_in="secW")
tax = t.rotation_axis(t.origin, (1, 0))
t.horizontal(tax)
wrep = wheel.build(at("wheel.wpc"))
t.project("hub_rim", wrep.edges(where=lambda e: e["straight"] and e["middle"][2] > 19 and abs(e["middle"][0]) < 1e-6)[0])
q0, q1, q2, q3 = t.point(-15, 20, "a"), t.point(15, 20, "b"), t.point(15, 37.5, "c"), t.point(-15, 37.5, "d")
for u, v in ((q0, q1), (q1, q2), (q2, q3), (q3, q0)):
    t.line(u, v)
t.horizontal(q0, q1); t.vertical(q1, q2); t.horizontal(q2, q3); t.vertical(q3, q0)
t.coincident(q0, "hub_rim_a"); t.coincident(q1, "hub_rim_b")
t.diameter_about(q3, tax, 75)
wheel.revolve(t, combine="NewBody", id="tyre")
wrep = wheel.build(at("wheel.wpc"))
wheel.fillet(wrep.edges(where=lambda e: not e["straight"] and e["name"].startswith("tyre") and abs(abs(e["middle"][1]) - 15) < 1e-6
                        and abs(e["middle"][0]) + abs(e["middle"][2]) > 30), 6, id="tyre_round")
wheel.op("Appearance", id="rubber", of=[topo("tyre")], material="Rubber")
wheel.op("Section", id="W", along="Plane", plane=axis("X"), offset=0.0)
wheel.drawing(views=[view("right", "Right", (100, 165)), section_view("secW", "W", (230, 165), of="right"),
                     view("iso", "Isometric", (335, 215))], scale=(1, 1), format="A3", id="drawing")

# --- The axle pin: Ø8 × 56 under a Ø14 × 4 head.
# (A pin is drawn whole where a section runs along it: not sectioned.)
pin = Part(material="Steel", title="AXLE PIN", file="pin.wpc", sectioned=False)
pp = pin.sketch("YZ", id="pin_profile", show_in="front")
pax = pp.rotation_axis(pp.origin, (1, 0))
pp.horizontal(pax)
pts = [pp.point(u, v, f"p{i}") for i, (u, v) in enumerate([(0, 0), (0, 7), (4, 7), (4, 4), (60, 4), (60, 0)])]
for i in range(6):
    pp.line(pts[i], pts[(i + 1) % 6])
    (pp.vertical if i % 2 == 0 else pp.horizontal)(pts[i], pts[(i + 1) % 6])
pp.coincident(pts[0], pp.origin)
pp.diameter_about(pts[1], pax, 14)
pp.diameter_about(pts[3], pax, 8)
pp.distance_u(pts[1], pts[2], 4)
pp.distance_u(pts[0], pts[5], 60)
pin.revolve(pp, combine="NewBody", id="pin")
pin.drawing(views=[view("front", "Front", (150, 170)), view("iso", "Isometric", (320, 220))],
            scale=(2, 1), format="A3", id="drawing")

# --- The assembly.
frep, wrep, prep = fork.build(at("fork.wpc")), wheel.build(at("wheel.wpc")), pin.build(at("pin.wpc"))
circle = lambda rep, y, r: rep.edges(where=lambda e: not e["straight"] and abs(e["middle"][1] - y) < 1e-6
                                     and abs(((e["middle"][0]) ** 2 + (e["middle"][2] - (20 if rep is frep else 0)) ** 2) ** .5 - r) < 1e-3)[0]
asm = Part(title="CASTER")
asm.add_part(fork, id="fork")
asm.add_part(pin, id="pin", constraint="Fastened", flip=True,
             onto=anchor(topo(circle(frep, -25, 4)), "ArcCentre", instance="fork"),
             with_=anchor(topo(circle(prep, 4, 4)), "ArcCentre"))
asm.add_part(wheel, id="wheel", constraint="Revolute", flip=True,
             onto=anchor(topo(circle(frep, -22, 4)), "ArcCentre", instance="fork"),
             with_=anchor(topo(circle(wrep, -15, 4)), "ArcCentre"), offset=(0, 0, -7))
asm.op("Section", id="A", along="Plane", plane=axis("X"), offset=0.0)
asm.drawing(views=[view("right", "Right", (90, 175)), section_view("secA", "A", (215, 175), of="right"),
                   view("iso", "Isometric", (335, 215))], scale=(1, 1), format="A3", id="drawing",
            parts_list=True, balloons="auto")
rep = asm.build(at("caster.wpc"), pictures=True, render=True, picture=at("render.png"))
for f, r in asm.reports.items():
    r.show()
rep.show()
