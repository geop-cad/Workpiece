"""Small bench vise: a cast body with a fixed jaw, a moving jaw that slides on the body, and an M12
screw that turns in the moving jaw and passes through the fixed jaw.

The moving jaw is held by a slider joint against the fixed jaw's face, the screw by a revolute joint
in the jaw's seat; a Drive step opens the jaws to a width. The sheet shows the vise opened 40, a
section through the screw (shown whole: it is not sectioned), a parts list and balloons; the same
assembly driven to 8 is built beside it (`bench_vise_closed.workpiece`, `render-closed.png`), to
show it moves.

    python3 bench_vise.py            # opened 40 (and closed to 8)
    python3 bench_vise.py OPEN=25    # opened 25
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, anchor, axis, balloon, topo, view, projected, section_view

HERE = os.path.dirname(os.path.abspath(__file__))
at = lambda name: os.path.join(HERE, name)
OPEN = 40.0  # how far apart the jaws are, mm
for arg in sys.argv[1:]:
    name, value = arg.split("=")
    if name == "OPEN":
        OPEN = float(value)


def profile(part, id, corners, show_in):
    """A closed outline on ZX (u = z, v = x) through `corners`, each side along u or v, its first
    corner on the origin."""
    s = part.sketch("ZX", id=id, show_in=show_in)
    pts = [s.point(u, v, f"p{i}") for i, (u, v) in enumerate(corners)]
    for i in range(len(pts)):
        s.line(pts[i], pts[(i + 1) % len(pts)])
        a, b = corners[i], corners[(i + 1) % len(corners)]
        (s.horizontal if a[1] == b[1] else s.vertical)(pts[i], pts[(i + 1) % len(pts)])
    s.coincident(pts[0], s.origin)
    return s, pts


# --- The body: a base 140 long, 60 wide, 12 thick, the fixed jaw 18 thick standing 38 on it at x = 0;
# a Ø13 bore for the screw through the jaw, 31 up.
body = Part(material="Cast iron", title="VISE BODY", file="body.workpiece")
s, q = profile(body, "body_profile", [(0, 0), (0, 140), (12, 140), (12, 18), (50, 18), (50, 0)], None)
s.distance_v(q[0], q[1], 140)
s.distance_u(q[1], q[2], 12)
s.distance_v(q[5], q[4], 18)
s.distance_u(q[0], q[5], 50)
body.extrude(s, 60, symmetric=True, combine="NewBody", id="body")
brep = body.build(at("body.workpiece"))
outside = brep.faces(where=lambda f: f["flat"] and f["normal"][0] < -0.99)[0]
b = body.sketch(topo(outside), id="bore_place", show_in="left")
dot = b.dot(0, 31, "c")
b.point_on(dot, "v")
b.distance_v(b.origin, dot, 31)
body.hole(b, 13, next=True, id="bore")
body.drawing(views=[view("front", "Front", (230, 110)), projected("top", "front", "Above", 75),
                    projected("left", "front", "Left", 120), view("iso", "Isometric", (330, 215))],
             scale=(1, 1), format="A3", id="drawing")

# --- The moving jaw: 18 thick, 60 wide, 38 high; a Ø12 seat 8 deep for the screw's tip on its back,
# 19 up.
jaw = Part(material="Cast iron", title="MOVING JAW", file="jaw.workpiece")
s, q = profile(jaw, "jaw_profile", [(0, 0), (0, 18), (38, 18), (38, 0)], None)
s.distance_v(q[0], q[1], 18)
s.distance_u(q[1], q[2], 38)
jaw.extrude(s, 60, symmetric=True, combine="NewBody", id="jaw")
jrep = jaw.build(at("jaw.workpiece"))
back = jrep.faces(where=lambda f: f["flat"] and f["normal"][0] < -0.99)[0]
k = jaw.sketch(topo(back), id="seat_place", show_in="left")
dot = k.dot(0, 19, "c")
k.point_on(dot, "v")
k.distance_v(k.origin, dot, 19)
jaw.hole(k, 12, depth=8, point=180, id="seat")
jaw.drawing(views=[view("front", "Front", (120, 160)), projected("left", "front", "Left", 70),
                   view("iso", "Isometric", (320, 200))], scale=(2, 1), format="A3", id="drawing")

# --- The screw: Ø12 × 110 with an M12 thread, a Ø24 × 12 head; turned about x (it lies as it is placed). A solid screw is
# drawn whole where a section runs along it.
screw = Part(material="Steel", title="VISE SCREW", file="screw.workpiece", sectioned=False)
t = screw.sketch("XY", id="screw_profile", show_in="top")
axis_line = t.rotation_axis(t.origin, (1, 0))
t.horizontal(axis_line)
corners = [(0, 0), (0, 12), (12, 12), (12, 6), (122, 6), (122, 0)]
pts = [t.point(u, v, f"p{i}") for i, (u, v) in enumerate(corners)]
for i in range(6):
    t.line(pts[i], pts[(i + 1) % 6])
    (t.vertical if i % 2 == 0 else t.horizontal)(pts[i], pts[(i + 1) % 6])
t.coincident(pts[0], t.origin)
t.diameter_about(pts[1], axis_line, 24)
t.diameter_about(pts[3], axis_line, 12, on_drawing=False)  # (the thread's callout says M12)
t.distance_u(pts[1], pts[2], 12)
t.distance_u(pts[0], pts[5], 122)
screw.revolve(t, combine="NewBody", id="screw")
srep = screw.build(at("screw.workpiece"))
shank = srep.faces(where=lambda f: f["surface"] == "cylinder" and f["middle"][0] > 20)[0]
screw.op("Thread", id="thread", face=topo(shank))
screw.on_sheet("thread/callout", show_in="top", at=(40, -30))
screw.drawing(views=[view("top", "Top", (160, 170)), view("iso", "Isometric", (330, 230))],
              scale=(1, 1), format="A3", id="drawing")

# --- The vise: the body fixed; the jaw slides on it, its back face against the fixed jaw's inner face
# (the slider's travel is how far apart they are); the screw turns in the jaw's seat; Drive opens it.
brep, jrep, srep = body.build(at("body.workpiece")), jaw.build(at("jaw.workpiece")), screw.build(at("screw.workpiece"))
inner = brep.faces(where=lambda f: f["flat"] and f["normal"][0] > 0.99 and f["middle"][0] < 20)[0]
seat_floor = jrep.faces(where=lambda f: f["flat"] and f["name"].startswith("seat/"))[0]
tip = srep.faces(where=lambda f: f["flat"] and f["middle"][0] > 100)[0]
def vise(open_at: float, drawing: bool) -> Part:
    """The vise with its jaws `open_at` apart; with its sheet, if `drawing`."""
    asm = Part(title="BENCH VISE")
    asm.add_part(body, id="body")
    asm.add_part(jaw, id="jaw", constraint="Slider", flip=True,
                 onto=anchor(topo(inner), "FaceCentre", instance="body"), with_=anchor(topo(back), "FaceCentre"))
    asm.add_part(screw, id="screw", constraint="Revolute", flip=True,
                 onto=anchor(topo(seat_floor), "FaceCentre", instance="jaw"), with_=anchor(topo(tip), "FaceCentre"))
    asm.op("Drive", "open", joint="jaw", at=open_at)
    if drawing:
        asm.op("Section", id="A", along="Plane", plane=axis("Y"), offset=0.0)
        asm.drawing(views=[section_view("secA", "A", (160, 185), of="top", caption_at=(0, -32), scale=(1, 1)),
                           view("top", "Top", (300, 150), scale=(1, 2)),
                           view("iso", "Isometric", (360, 235), scale=(1, 2))],
                    scale=(1, 1), format="A3", id="drawing", parts_list=True,
                    balloons=[balloon("body", "secA", (-40, 55)), balloon("jaw", "secA", (30, 55)),
                              balloon("screw", "secA", (-80, 30))])
    return asm


asm = vise(OPEN, drawing=True)
rep = asm.build(at("bench_vise.workpiece"), render=True, pictures=True, picture=at("render.png"))
for part_report in asm.reports.values():
    part_report.show()
rep.show()

# The same vise driven nearly shut: only where the jaw (and the screw in it) is changes.
closed = vise(8.0, drawing=False).build(at("bench_vise_closed.workpiece"), picture=at("render-closed.png"))
print("closed: jaw at", [round(c, 3) for c in closed.placed_part("jaw")["pose"]["at"]])
