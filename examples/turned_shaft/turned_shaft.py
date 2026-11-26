"""Turned drive shaft: six diameters on one revolved profile, an M16 thread, a keyway, chamfers.

The profile is drawn on YZ with the axis along y, so the front view shows the shaft lying down,
as a turned part is drawn. Lengths are chained from the threaded end; the keyway is cut down to a
datum plane at its floor and shown in a section.
"""
import math, os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, axis, topo, view, projected, section_view, note, datum

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "turned_shaft.wpc")
p = Part(material="Steel", title="DRIVE SHAFT", file="turned_shaft.wpc")

# Half the profile, above the axis: (length along the shaft, radius) as a starting guess.
s = p.sketch("YZ", id="profile", show_in="front")
ax = s.rotation_axis(s.origin, (1, 0))
s.horizontal(ax)
guess = [(0, 0), (0, 8), (20, 8), (20, 10), (45, 10), (45, 13), (55, 13), (55, 11), (115, 11),
         (115, 10), (135, 10), (135, 8), (160, 8), (160, 0)]
pts = [s.point(u, v, f"p{i}") for i, (u, v) in enumerate(guess)]
for i in range(len(pts)):
    s.line(pts[i], pts[(i + 1) % len(pts)])
    (s.vertical if i % 2 == 0 else s.horizontal)(pts[i], pts[(i + 1) % len(pts)])
s.coincident(pts[0], s.origin)
# Each step's diameter, and its length chained from the threaded end.
# (Each diameter's text over the middle of its own step, mm on the sheet from the view's middle.)
# The threaded end's Ø16 is kept off the sheet: the thread's callout says M16 there.
# Fits (ISO 286): k6 on both bearing seats and the coupling seat, the hub seat under the key (a light press).
fits = {3: "k6", 7: "k6", 9: "k6", 11: "k6"}
for i, dia, x in ((1, 16, -140), (3, 20, -95), (5, 26, -60), (7, 22, -20), (9, 20, 90), (11, 16, 135)):
    s.diameter_about(pts[i], ax, dia, at=(x, 0), on_drawing=i != 1, **({"tol": fits[i]} if i in fits else {}))
for i, length in ((1, 20), (3, 25), (5, 10), (7, 60), (9, 20), (11, 25)):
    s.distance_u(pts[i], pts[i + 1], length)
p.revolve(s, combine="NewBody", id="shaft")
rep = p.build(OUT)

# The keyway: 6 wide, 40 long, its floor 3.5 below the top of the Ø22 (a plane there), 10 in from the collar.
floor = p.plane("Z", 7.5, id="keyway_floor")
k = p.sketch(datum(floor), id="keyway", show_in="top")
c0, c1 = k.point(0, 68, "c0"), k.point(0, 102, "c1")
a0, b0, a1, b1 = k.point(3, 68, "a0"), k.point(-3, 68, "b0"), k.point(3, 102, "a1"), k.point(-3, 102, "b1")
e0, e1 = k.point(0, 65, "e0"), k.point(0, 105, "e1")
la, lb = k.line(a0, a1, "la"), k.line(b0, b1, "lb")
arc0, arc1 = k.arc(c0, b0, a0, "arc0"), k.arc(c1, a1, b1, "arc1")
for line in (la, lb):
    for arc in (arc0, arc1):
        k.tangent(line, arc)
k.equal(arc0, arc1)
k.point_on(c0, "v"); k.point_on(c1, "v")
k.point_on(e0, arc0); k.point_on(e1, arc1); k.point_on(e0, "v"); k.point_on(e1, "v")
k.point_line(a0, lb, 6, id="width", at=(0, 22), show_in="secB", tol="N9")  # key width, a normal fit for the key
k.distance_v(e0, e1, 40, id="length")
k.distance_v(k.origin, e0, 65, id="from_end")
p.cut(k, through=True, id="keyway_cut")

# Chamfers on both ends, a thread on the Ø16 end, small rounds at the collar.
rep = p.build(OUT)
ends = rep.edges(where=lambda e: not e["straight"] and (abs(e["middle"][1]) < 1e-6 or abs(e["middle"][1] - 160) < 1e-6))
p.chamfer(ends, 1, id="end_chamfers")
r = lambda e: math.hypot(e["middle"][0], e["middle"][2])
corners = rep.edges(where=lambda e: not e["straight"] and ((abs(e["middle"][1] - 45) < 1e-6 and abs(r(e) - 10) < 1e-3)
                                                       or (abs(e["middle"][1] - 55) < 1e-6 and abs(r(e) - 11) < 1e-3)))
p.fillet(corners, 1, id="collar_rounds")
rep = p.build(OUT)
thread_face = rep.faces(where=lambda f: not f["flat"] and 0 < f["middle"][1] < 20 and f["name"].startswith("shaft"))
p.op("Thread", id="thread", face=topo(thread_face[0]))


# The sheet at 2:1: the shaft lying down, the plan above it with the keyway, a section through the keyway.
key_floor = rep.faces(where=lambda f: f["flat"] and f["name"].startswith("keyway_cut") and abs(f["normal"][2]) > 0.99)[0]
bottom = rep.faces(where=lambda f: not f["flat"] and 55 < f["middle"][1] < 115 and f["name"].startswith("shaft"))[0]
p.on_sheet("keyway_floor", on_drawing=False)
p.on_sheet("end_chamfers", on_drawing=False)
end_faces = rep.faces(where=lambda f: f["flat"] and abs(f["normal"][1]) > 0.99 and min(abs(f["middle"][1]), abs(f["middle"][1] - 160)) < 1e-6)
p.dimension(*end_faces, show_in="top", reference=True, id="overall", at=(0, 56))
p.dimension(key_floor, bottom, far=True, show_in="secB", id="key_depth", at=(-32, 0))
p.on_sheet("thread/callout", show_in="front", at=(-100, -58))
p.op("Section", id="B", along="Plane", plane=axis("Y"), offset=85.0)
p.drawing(views=[view("front", "Front", (180, 115)), projected("top", "front", "Above", 85),
                 section_view("secB", "B", (380, 205), of="front", letter="B")],
          scale=(2, 1), format="A3", id="drawing",
          notes=[note("front", (-136, -48), "1×45°", to=(0, 0.5, -7.5)),
                 note("front", (150, -50), "1×45°", to=(0, 159.5, -7.5))])
rep = p.build(OUT, render=True, pictures=True, picture=os.path.join(HERE, "render.png"))
rep.show()
