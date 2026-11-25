"""Sheet-metal cover, driven by named values: a base, two flanges with inward lips, a row of vent
slots, four mounting holes, and its flat pattern on a sheet of its own.

Change a value on the command line and the cover, its drawings and its flat pattern follow:

    python3 sheet_metal_cover.py LENGTH=200 SLOTS=9
"""
import os, sys
try:
    import workpiece
except ImportError:  # a source checkout: the package's sources
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))
from workpiece import Part, axis, note, topo, view, projected

# The named values the cover is built from (mm); NAME=value on the command line overrides one.
values = dict(LENGTH=160, WIDTH=100, HEIGHT=40, LIP=12, THICKNESS=1.5, BEND_RADIUS=2,
              SLOTS=7, SLOT_PITCH=16, SLOT_INSET=32, SLOT_LENGTH=50, SLOT_WIDTH=5, HOLE=4.5, HOLE_INSET=20)
for arg in sys.argv[1:]:
    name, value = arg.split("=")
    values[name] = type(values[name])(value)
globals().update(values)

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "sheet_metal_cover.workpiece")
p = Part(material="Steel", title="VENTED COVER", file="sheet_metal_cover.workpiece")

# The base: LENGTH along x, WIDTH along y, THICKNESS thick.
s = p.sketch("XY", id="base_outline", show_in="top")
r = s.rect("b", -LENGTH / 2, -WIDTH / 2, LENGTH, WIDTH)
s.symmetric(r.points[0], r.points[1], "v")
s.symmetric(r.points[1], r.points[2], "u")
s.distance_u(r.points[0], r.points[1], LENGTH)
s.distance_v(r.points[1], r.points[2], WIDTH)
p.extrude(s, THICKNESS, combine="NewBody", id="base")

# One vent slot across the middle, SLOT_INSET from the left end, then a row of SLOTS along x.
rep = p.build(OUT)
top_edge = lambda x=None, y=None: rep.edges(where=lambda e: e["straight"] and abs(e["middle"][2] - THICKNESS) < 1e-6 and (
    abs(e["middle"][0] - x) < 1e-6 if x is not None else abs(e["middle"][1] - y) < 1e-6))[0]
v = p.sketch(topo("base/end"), id="vent", show_in="top")
v.project("left", top_edge(x=-LENGTH / 2))
sl = v.slot("s", -LENGTH / 2 + SLOT_INSET, -SLOT_LENGTH / 2 + SLOT_WIDTH / 2, SLOT_LENGTH - SLOT_WIDTH,
            SLOT_WIDTH, upright=True)
c0, c1 = sl.points[0], sl.points[1]
v.vertical(c0, c1)
v.symmetric(c0, c1, "u")
v.point_line(c0, "left", SLOT_INSET)
p.cut(v, through=True, id="slot")
rep = p.build(OUT)
slot_wall = rep.faces(where=lambda f: f["name"].startswith("slot/") and not f["flat"])[0]
p.feature_pattern([slot_wall], SLOTS, along=axis("X"), spacing=SLOT_PITCH, id="vents")

# Four mounting holes, HOLE_INSET in from the base's corners: one hole step at four dots.
h = p.sketch(topo("base/end"), id="holes", show_in="top")
corner = [(1, 1), (-1, 1), (-1, -1), (1, -1)]
dots = [h.dot(sx * (LENGTH / 2 - HOLE_INSET), sy * (WIDTH / 2 - HOLE_INSET), f"h{i}") for i, (sx, sy) in enumerate(corner)]
h.symmetric(dots[0], dots[1], "v"); h.symmetric(dots[1], dots[2], "u"); h.symmetric(dots[2], dots[3], "v")
h.project("end", top_edge(x=LENGTH / 2))
h.project("side", top_edge(y=WIDTH / 2))
h.point_line(dots[0], "end", HOLE_INSET)
h.point_line(dots[0], "side", HOLE_INSET)
p.hole(h, HOLE, id="mount")

# The flanges, bent up along both long edges, and a lip bent inwards at the top of each.
rep = p.build(OUT)
long_edges = rep.edges(where=lambda e: e["straight"] and abs(e["middle"][2] - THICKNESS) < 1e-6
                       and abs(abs(e["middle"][1]) - WIDTH / 2) < 1e-6 and abs(e["middle"][0]) < 1e-6)
for k, edge in enumerate(long_edges):
    p.op("Flange", id=f"side{k + 1}", edge=topo(edge), thickness=THICKNESS, radius=BEND_RADIUS, angle=90.0,
         length=HEIGHT)
rep = p.build(OUT)
for k in (1, 2):
    tip = rep.edges(where=lambda e: e["name"].startswith(f"side{k}/along/") and abs(e["middle"][2] - HEIGHT - THICKNESS - BEND_RADIUS) < 1e-6
                    and abs(e["middle"][1]) < WIDTH / 2 + BEND_RADIUS + THICKNESS - 1e-6)[0]
    p.op("Flange", id=f"lip{k}", edge=topo(tip), thickness=THICKNESS, radius=BEND_RADIUS, angle=90.0, length=LIP)


# The cover's sheet: the plan with the vents and holes, the channel seen end on with its outside
# sizes and bend radius, and a pictorial.
rep = p.build(OUT)
face = lambda name: rep.faces(where=lambda f: f["name"] == name)[0]
p.dimension("base/start/e11", "lip1/side/e2", show_in="front", id="overall_height", at=(-64, 0))
p.dimension("side1/side/e2", "side2/side/e2", show_in="front", id="overall_width")
p.dimension("lip2/side/e3", "side2/side/e2", show_in="front", id="lip_width")
p.dimension("side2/side/e5", kind="Radius", show_in="front", id="bend")
p.on_sheet("base", show_in="front")
p.drawing(views=[view("top", "Top", (125, 190)), projected("front", "top", "Below", 125),
                 view("iso", "Isometric", (320, 200), scale=(1, 2))], scale=(1, 1), format="A3", id="drawing")

# The flat pattern: the sheet laid out in the base's plane, a body of its own, on a sheet of its own
# with its developed width; then taken away again, so that the part weighs as the cover alone.
p.op("Unfold", id="flat", face=topo("base/end"), thickness=THICKNESS)
rep = p.build(OUT)
flat = next(b for b in rep.raw["bodies"] if b["name"] == "flat")
long_sides = [f["name"] for f in flat["faces"] if abs(f["normal"][1]) > 0.99 and abs(f["middle"][1]) > WIDTH / 2 + HEIGHT]
p.dimension(*long_sides, show_in="top", id="developed", decimals=1, at=(0, 112))
for done in ("overall_height", "overall_width", "lip_width", "bend", "base", "vent", "vents", "holes", "mount"):
    p.on_sheet(done, on_drawing=False)
# Its bend lines (the Unfold step's `flat/bend1`, ...), each said how it is bent.
# (Their words above the pattern: each line runs the way its bend was found.)
bends = [{"of": f"flat/bend{k}", "view": "top", "label": f"UP 90° R{BEND_RADIUS:g}", "label_at": at}
         for k, at in ((1, "start"), (2, "end"), (3, "start"), (4, "end"))]
p.drawing(views=[view("top", "Top", (150, 165), bodies=["flat"])], scale=(1, 1), format="A3", id="flat_pattern",
          centre_lines=["all"] + bends,
          notes=[note("top", (0, -125), f"FLAT PATTERN  K = 0,44  SHEET {THICKNESS:g} THICK".replace(".", ","))])
p.op("DeleteBody", id="flat_gone", bodies=[topo("flat")])

rep = p.build(OUT, render=True, pictures=True, picture=os.path.join(HERE, "render.png"))
rep.show()
