"""Bearing unit: a cast housing, a stepped shaft running in two 6204 ball bearings, a cover held by
four M5 socket head cap screws — three modelled parts and catalogue parts, put together by joints.

The housing, shaft and cover are parts of their own, each with its drawing. The assembly fixes the
housing, fastens each bearing against its shoulder, holds the shaft in the bearings with a revolute
joint (it turns), fastens the cover on the housing's face and the first screw in the cover, and
patterns that screw four times about the axis. Its sheet is a section through the axis with a parts
list and balloons.
"""
import math, os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, anchor, axis, balloon, note, topo, view, section_view

HERE = os.path.dirname(os.path.abspath(__file__))


def turned(part, id, profile, show_in):
    """A half profile on YZ about the y axis, (y, radius) corners in order, closed along the axis
    at its ends: lines alternate along and across, each step a diameter about the drawn axis."""
    s = part.sketch("YZ", id=id, show_in=show_in)
    ax = s.rotation_axis(s.origin, (1, 0))
    s.horizontal(ax)
    pts = [s.point(u, v, f"p{i}") for i, (u, v) in enumerate(profile)]
    for i in range(len(pts)):
        s.line(pts[i], pts[(i + 1) % len(pts)])
    return s, ax, pts


# --- The housing: a flange at its front (y = 0) and a body behind, two Ø47 seats with a Ø36 bore
# between them; four tapped M5 holes in the flange on a Ø88 pitch circle.
housing = Part(material="Cast iron", title="HOUSING", file="housing.wpc")
s, ax, q = turned(housing, "housing_profile", [(0, 23.5), (0, 50), (12, 50), (12, 40), (60, 40), (60, 23.5),
                                               (46, 23.5), (46, 18), (14, 18), (14, 23.5)], "secA")
for i in range(10):
    (s.vertical if i % 2 == 0 else s.horizontal)(q[i], q[(i + 1) % 10])
s.point_on(q[0], "v")
s.horizontal(q[0], q[5])                  # both seats alike
s.diameter_about(q[1], ax, 100)
s.diameter_about(q[3], ax, 80)
s.diameter_about(q[0], ax, 47)
s.diameter_about(q[7], ax, 36)
s.distance_u(q[1], q[2], 12)
s.distance_u(q[0], q[5], 60)
s.distance_u(q[9], q[0], 14)
s.distance_u(q[6], q[5], 14)
housing.revolve(s, combine="NewBody", id="housing")
hrep = housing.build(os.path.join(HERE, "housing.wpc"))
front = hrep.faces(where=lambda f: f["normal"][1] < -0.99 and abs(f["middle"][1]) < 1e-6)[0]
t = housing.sketch(topo(front), id="tap_place", show_in="left")
dot = t.dot(0, 44, "tap")
t.point_on(dot, "v")
t.diameter_about(dot, "u", 88)            # on the Ø88 pitch circle
housing.hole(t, 5, depth=10, kind="Tapped", size="M5", id="tap")
hrep = housing.build(os.path.join(HERE, "housing.wpc"))
housing.feature_pattern([hrep.faces(where=lambda f: f["name"].startswith("tap/") and not f["flat"])[0]], 4,
                        about=axis("Y"), id="taps")
housing.op("Section", id="HA", along="Plane", plane=axis("X"), offset=0.0)
housing.drawing(views=[view("left", "Left", (85, 165)),
                       section_view("secA", "HA", (205, 165), of="left", caption_at=(0, -66)),
                       view("iso", "IsometricFrontLeft", (330, 205))], scale=(1, 1), format="A3", id="drawing",
                centre_lines=["all", ("pitch", "taps"), {"of": "origin/y", "view": "secA"}])

# --- The shaft: Ø20, a Ø26 collar between the bearings, chamfered ends.
# (A solid shaft is drawn whole where a section runs along it: not sectioned.)
shaft = Part(material="Steel", title="SHAFT", file="shaft.wpc", sectioned=False)
s, ax, q = turned(shaft, "shaft_profile", [(0, 0), (0, 10), (49, 10), (49, 13), (81, 13), (81, 10), (130, 10), (130, 0)],
                  "front")
for i in range(8):
    (s.vertical if i % 2 == 0 else s.horizontal)(q[i], q[(i + 1) % 8])
s.coincident(q[0], s.origin)
s.horizontal(q[1], q[6])                  # both journals alike
s.diameter_about(q[1], ax, 20, at=(-81, 0))
s.diameter_about(q[3], ax, 26, at=(0, 0))
s.distance_u(q[1], q[2], 49)
s.distance_u(q[3], q[4], 32)
s.distance_u(q[0], q[7], 130)
shaft.revolve(s, combine="NewBody", id="shaft")
srep = shaft.build(os.path.join(HERE, "shaft.wpc"))
shaft.chamfer(srep.edges(where=lambda e: not e["straight"] and min(abs(e["middle"][1]), abs(e["middle"][1] - 130)) < 1e-6), 1,
              id="ends")
shaft.on_sheet("ends", on_drawing=False)
shaft.drawing(views=[view("front", "Front", (160, 160)), view("iso", "Isometric", (330, 235), scale=(1, 1))],
              scale=(2, 1), format="A3", id="drawing", notes=[note("front", (-120, -45), "1×45°", to=(0, 0.5, -9.5))])

# --- The cover: Ø100 × 8 with a Ø24 opening for the shaft, four Ø5,5 holes on the Ø88 pitch circle.
cover = Part(material="Aluminium", title="COVER", file="cover.wpc")
c = cover.sketch("ZX", id="cover_outline", show_in="left")
cover_rim = c.circle(c.origin, 50, "rim")
opening = c.circle(c.origin, 12, "opening")
c.diameter(cover_rim, 100)
c.diameter(opening, 24)
cover.extrude(c, 8, combine="NewBody", flip=True, id="cover")
crep = cover.build(os.path.join(HERE, "cover.wpc"))
outer = crep.faces(where=lambda f: f["normal"][1] < -0.99)[0]
h = cover.sketch(topo(outer), id="hole_place", show_in="left")
hd = h.dot(0, 44, "h")
h.point_on(hd, "v")
h.diameter_about(hd, "u", 88)
cover.hole(h, 5.5, id="screw_hole")
crep = cover.build(os.path.join(HERE, "cover.wpc"))
cover.feature_pattern([crep.faces(where=lambda f: f["name"].startswith("screw_hole/") and not f["flat"])[0]], 4,
                      about=axis("Y"), id="screw_holes")
cover.op("Section", id="CA", along="Plane", plane=axis("X"), offset=0.0)
cover.drawing(views=[view("left", "Left", (90, 165)),
                     section_view("secA", "CA", (190, 165), of="left", caption_at=(0, -66)),
                     view("iso", "IsometricFrontLeft", (320, 205))], scale=(1, 1), format="A3", id="drawing",
              centre_lines=["all", ("pitch", "screw_holes"), {"of": "origin/y", "view": "secA"}])

# --- The assembly.
asm = Part(title="BEARING UNIT")
asm.add_part(housing, id="housing")
hrep = housing.build(os.path.join(HERE, "housing.wpc"))
srep = shaft.build(os.path.join(HERE, "shaft.wpc"))
crep = cover.build(os.path.join(HERE, "cover.wpc"))
face_at = lambda rep, ny, y: rep.faces(where=lambda f: abs(f["normal"][1] - ny) < 1e-6 and abs(f["middle"][1] - y) < 1e-6)[0]
# The bearings, each against its shoulder in the housing.
for k, shoulder in enumerate((face_at(hrep, -1, 14), face_at(hrep, 1, 46))):
    asm.op("StandardPart", f"bearing{k + 1}", kind="Bearing", size="6204",
           hold={"constraint": {"Joint": "Fastened"}, "flip": True,
                 "onto": anchor(topo(shoulder), "FaceCentre", instance="housing"), "with": anchor(topo(f"bearing{k + 1}/end"), "FaceCentre")})
# The shaft turns in them: its collar's shoulder (a revolved annulus) against the first bearing's face.
asm.add_part(shaft, id="shaft", constraint="Revolute", flip=True,
             onto=anchor(topo("bearing1/end"), "FaceCentre", instance="bearing1"),
             with_=anchor(topo(face_at(srep, -1, 49)), "FaceCentre"))
# The cover on the housing's face, one screw in it, and the screw four times about the axis.
asm.add_part(cover, id="cover", constraint="Fastened", flip=True,
             onto=anchor(topo(face_at(hrep, -1, 0)), "FaceCentre", instance="housing"),
             with_=anchor(topo(face_at(crep, 1, 0)), "FaceCentre"))
asm.op("StandardPart", "screw", kind="SocketScrew", size="M5", length=12.0,
       hold={"constraint": {"Joint": "Fastened"}, "flip": True, "offset": [0.0, 44.0, 0.0],
             "onto": anchor(topo(face_at(crep, -1, -8)), "FaceCentre", instance="cover"),
             "with": anchor(topo("screw/head/start"), "FaceCentre")})
rep = asm.build(os.path.join(HERE, "bearing_unit.wpc"))


def quarter_turn_about_y(pose):
    """The step, in the screw's own frame, that takes it a quarter turn on about the world's y axis
    (a part pattern's step is a move and turn of each copy from the one before, in its own frame)."""
    def mul(a, b):
        (ax, ay, az, aw), (bx, by, bz, bw) = a, b
        return [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
    def turn(q, v):
        return mul(mul(q, [*v, 0.0]), [-q[0], -q[1], -q[2], q[3]])[:3]
    q, p = pose["rotation"], pose["at"]
    inv = [-q[0], -q[1], -q[2], q[3]]
    r = [0.0, math.sin(math.pi / 4), 0.0, math.cos(math.pi / 4)]
    moved = turn(r, p)
    return {"rotation": mul(mul(inv, r), q), "translation": turn(inv, [m - a for m, a in zip(moved, p)])}


asm.op("PatternPart", "screws", of="screw", count=4, step=quarter_turn_about_y(rep.placed_part("screw")["pose"]))
asm.op("Section", id="A", along="Plane", plane=axis("X"), offset=0.0)
asm.drawing(views=[view("left", "Left", (80, 175)), section_view("secA", "A", (205, 175), of="left", caption_at=(0, -66)),
                   view("iso", "IsometricFrontLeft", (355, 215), scale=(1, 2))],
            scale=(1, 1), format="A3", id="drawing", parts_list=True,
            centre_lines=["all", {"of": "origin/y", "view": "secA"}],
            balloons=[balloon("screw", "secA", (-62, 64)), balloon("bearing1", "secA", (-30, 64)),
                      balloon("housing", "secA", (5, 64)), balloon("shaft", "secA", (80, 30)),
                      balloon("cover", "secA", (-62, -64))])
rep = asm.build(os.path.join(HERE, "bearing_unit.wpc"), render=True, pictures=True,
                picture=os.path.join(HERE, "render.png"))
for file, part_report in asm.reports.items():
    part_report.show()
rep.show()
