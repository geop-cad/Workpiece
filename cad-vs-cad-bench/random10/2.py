import math, os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from shapecraft import Part, axis, topo, datum, profile, view, projected, section_view, note, material, anchor, placed, balloon
here = os.path.dirname(os.path.abspath(__file__))
folder = os.path.join(here, "2.parts")
os.makedirs(folder, exist_ok=True)
material(folder, "ABS", 1020)
material(folder, "Red oak", 570)

# Origin of base and rack: the top-right corner of the 6 x 6 plates in the plan view (one of the corners with a rod
# hole); the drawing dimensions from there (.750 / 3.0). Each part is modelled standing on z = 0.
def outline(s):
    O = s.origin
    Y = s.point(0, -6, "Y"); W = s.point(6, -6, "W"); Q = s.point(6, -3, "Q"); P = s.point(3, 0, "P"); C = s.point(3, -3, "C")
    lOY = s.line(O, Y); lYW = s.line(Y, W); lWQ = s.line(W, Q); arc = s.arc(C, Q, P); lPO = s.line(P, O)
    s.vertical(lOY); s.horizontal(lYW); s.vertical(lWQ); s.horizontal(lPO)
    s.tangent(arc, lWQ); s.tangent(arc, lPO)
    s.radius(arc, 3.0, decimals=1, at=(48, -38))
    s.distance_v(Y, O, 6.0, decimals=1)
    s.distance_u(O, W, 6.0, decimals=1)

def plate(name, thick, floor_from, title):
    """A 6 x 6 plate with the Ø5 bore and a Ø.875 rod hole at each of three corners. The rod holes have a flat floor
    .5 from the face they do not open on: `floor_from` is "bottom" (the base: the holes open to the top, floor .5
    above the underside) or "top" (the rack: they open from below, floor .5 under the top)."""
    p = Part(unit="Inch", material="ABS", file=f"{name}.shapecraft", title=title)
    s = p.sketch("XY", id="outline", show_in="top")
    outline(s)
    p.extrude(s, thick, combine="NewBody", id=name)
    p.on_sheet(name, show_in="right", decimals=1 if name == "base" else 2)
    # Ø5 bore: the base's is a pocket whose floor is .5 above its underside, the rack's goes through.
    if floor_from == "bottom":
        fl = p.plane("Z", 0.5, id="floor")
    else:
        fl = p.plane(topo(f"{name}/end"), -0.5, id="floor")
    p.on_sheet("floor", show_in="right", decimals=3)
    bplane = datum(fl) if floor_from == "bottom" else "XY"
    b = p.sketch(bplane, id="bore_centre", show_in="top")
    bc = b.dot(3, -3)
    b.distance_u(b.origin, bc, 3.0, decimals=1)
    b.distance_v(bc, b.origin, 3.0, decimals=1)
    p.hole(b, 5.0, flip=True, id="bore")
    p.on_sheet("bore", show_in="front", decimals=1)
    # Rod holes Ø.875, .750 from the two edges of a corner, from their floor outwards; the other two by pattern about the bore axis.
    h = p.sketch(datum(fl), id="rod_hole", show_in="top")
    hc = h.dot(0.75, -0.75)
    h.distance_u(h.origin, hc, 0.75)
    h.distance_v(hc, h.origin, 0.75)
    p.hole(h, 0.875, flip=(floor_from == "bottom"), point=180.0, id="rod_holes")
    p.op("AddAxis", id="bore_axis", mode="Along", references=[topo("bore/side/e1")])
    p.feature_pattern(["rod_holes/side/e1"], 3, about=datum("bore_axis"), angle=180, id="rod_holes3")
    return p

base = plate("base", 2.0, "bottom", "LOWER BASE")
rack = plate("rack", 2.5, "top", "UPPER RACK")
rep = rack.build(os.path.join(folder, "rack.shapecraft")); rep.show()


rim = rep.edges(where=lambda e: abs(e["middle"][2] - 2.5) < 1e-6 and not e["straight"] and abs(math.hypot(e["middle"][0] - 3, e["middle"][1] + 3) - 2.5) < 1e-3)
rack.chamfer(rim, 0.25, id="lip")

def sheet(p, name):
    if name == "rack":
        p.on_sheet("lip", show_in="right")
    p.on_sheet("rod_holes3", on_drawing=False)
    p.on_sheet("bore_axis", on_drawing=False)
    p.on_sheet("rod_holes", on_drawing=False)
    p.dimension("rod_holes/side/e1", kind="Diameter", show_in="top", id="hole_dia", at=(100, 52))
    p.drawing(views=[view("front", "Front", (90, 85), style="HiddenDashed"),
                     projected("top", "front", "Above", 105, style="HiddenDashed"),
                     projected("right", "front", "Right", 110, style="HiddenDashed"),
                     view("iso", "Isometric", (330, 190))],
              scale=(1, 2), thru=(name != "base"), notes=[note(None, (0, 0), "HOLE (3) PLACES", dimension="hole_dia")] + ([note(None, (0, 0), "X 45° CHAMFER", dimension="lip")] if name == "rack" else []))
    rep = p.build(os.path.join(folder, p.file), render=True); rep.show()
    return rep

for p, n in ((base, "base"), (rack, "rack")):
    rep = sheet(p, n)

# The oak rod, turned from a profile that is the drawing's own numbers: 16.0 from the base's underside to the rack's top
# (the construction line), the rod .5 short of it at each end, Ø.875.
rod = Part(unit="Inch", material="Red oak", file="rod.shapecraft", title="WOODEN RODS")
r = rod.sketch("YZ", id="rod_profile", on_drawing=False)
span0 = r.point(0, -0.5); span1 = r.point(0, 15.5)
span = r.line(span0, span1, construction=True)
r.vertical(span); r.point_on(span0, "v")
r.distance_v(span0, span1, 16.0, decimals=1)
O = r.origin; A = r.point(0.4375, 0, "A"); B = r.point(0.4375, 15, "B"); C = r.point(0, 15, "C")
lOA = r.line(O, A); lAB = r.line(A, B); lBC = r.line(B, C); lCO = r.line(C, O)
r.horizontal(lOA); r.vertical(lAB); r.horizontal(lBC); r.vertical(lCO)
r.distance_v(span0, O, 0.5, decimals=3)
r.distance_v(C, span1, 0.5, decimals=3)
r.diameter_about(A, span, 0.875, decimals=3)
rod.revolve(r, about="SketchV", combine="NewBody", id="rod")
rod.on_sheet("rod", on_drawing=False)
rod.dimension("rod/side/e14", kind="Diameter", show_in="front", id="rod_dia")
rod.drawing(views=[view("front", "Front", (120, 150), style="HiddenDashed"), view("top", "Top", (300, 150))], scale=(1, 2))
rrep = rod.build(os.path.join(folder, "rod.shapecraft"), render=True); rrep.show()
for b in rrep.raw['bodies']:
    for f in b['faces']: print(f['name'], [round(x,2) for x in f['middle']], f['flat'])

# Assembly: the base fixed where it was modelled; each rod stands on the floor of one of the base's rod holes
# (its bottom face fastened onto the hole floor, centres together); the rack sits on the rods' tops, its first
# rod hole floor fastened onto the first rod's top face.
asm = Part(unit="Inch")
asm.add_part(base, id="base")
rb = asm.build(os.path.join(folder, "umbrella_stand.shapecraft"))
fl = rb.faces(placed="base", where=lambda f: f["flat"] and abs(f["middle"][2] - 0.5) < 1e-6 and f["area"] < 1.0)
rrep = rack.build(os.path.join(folder, "rack.shapecraft"))
rack_floor = rrep.faces(where=lambda f: f["flat"] and abs(f["middle"][2] - 2.0) < 1e-6 and f["area"] < 1.0 and f["name"].startswith("rod_holes/"))
print("hole floors", fl, rack_floor, rrep.faces(where=lambda f: f["flat"] and abs(f["middle"][2] - 2.0) < 1e-6 and f["area"] < 1.0))
for k, name in enumerate(fl):
    asm.add_part(rod, id=f"rod{k + 1}", constraint="Fastened", flip=True,
                 onto=anchor(topo(name), "FaceCentre", instance="base"),
                 with_=anchor(topo("rod/side/e13"), "FaceCentre"))
asm.add_part(rack, id="rack", constraint="Fastened", flip=True,
             onto=anchor(topo("rod/side/e15"), "FaceCentre", instance="rod1"),
             with_=anchor(topo(rack_floor[0]), "FaceCentre"))
rb = asm.build(os.path.join(folder, "umbrella_stand.shapecraft")); rb.show()
for pp in rb.placed: print(pp["name"], pp["pose"])
for m in rb.masses: print(m["name"], m["material"], m["mass"] * 2.20462, "lb")
print("total lb", rb.mass_total * 2.20462, "expected 3.061 +- 0.003")


# The assembly's sheet: the stand in front, top and right views, a section through two rods, and the pictorial view.
P = lambda inst, name: placed(inst, topo(name))
ROD = os.environ.get("ROD", "rod1")
hole = {"tr": "rod_holes/side/e1", "tl": "rod_holes3/1/rod_holes/side/e1", "bl": "rod_holes3/2/rod_holes/side/e1"}
asm.dimension(P("base", "base/side/e13"), P("base", "base/side/e15"), show_in="front", id="width_front", decimals=1)
asm.dimension(P("base", "base/side/e13"), P("base", "base/side/e15"), show_in="top", id="width_top_y", decimals=1, at=(0, -62))
asm.dimension(P("base", "base/side/e12"), P("base", "base/side/e14"), show_in="top", id="width_top_x", decimals=1, at=(-66, 0))
asm.dimension(P("base", "base/side/e12"), P("base", "base/side/e14"), show_in="right", id="width_right", decimals=1)
asm.dimension(P("base", "base/start"), P("rack", "rack/end"), show_in="front", id="overall", decimals=1)
asm.dimension(P("base", "base/start"), P("base", "base/end"), show_in="right", id="base_h", decimals=1, at=(27, -22))
asm.dimension(P("rack", "rack/start"), P("rack", "rack/end"), show_in="right", id="rack_h", decimals=2, at=(27, 20))
asm.dimension(P("base", "base/start"), P("base", fl[0]), show_in="right", id="floor", at=(-26, -22))
asm.dimension(P("rod1", "rod/side/e15"), P("rack", "rack/end"), show_in="right", id="rod_below_top", at=(-26, 22))
asm.dimension(P("rod2", "rod/side/e14"), kind="Diameter", show_in="front", id="rod_dia", at=(-42, 0))
asm.dimension(P("base", "bore/side/e1"), kind="Diameter", show_in="top", id="bore_dia", decimals=1, at=(64, -6))
asm.dimension(P("base", hole["tr"]), kind="Diameter", show_in="top", id="hole_dia", at=(64, 44))
asm.dimension(P("base", "base/side/e16"), kind="Radius", show_in="top", id="corner_r", decimals=1, at=(60, -42))
asm.dimension(P("base", "base/side/e13"), P("base", hole["tl"]), centres=True, show_in="top", id="hole_left_y", at=(-24, 50))
asm.dimension(P("base", "base/side/e15"), P("base", hole["tr"]), centres=True, show_in="top", id="hole_right_y", at=(26, 50))
asm.dimension(P("base", "base/side/e12"), P("base", hole["tl"]), centres=True, show_in="top", id="hole_top_x", at=(-48, 33))
asm.dimension(P("base", "base/side/e14"), P("base", hole["bl"]), centres=True, show_in="top", id="hole_bottom_x", at=(-48, -33))
asm.dimension(P("base", "base/side/e12"), P("base", "bore/side/e1"), centres=True, show_in="top", id="centre_x", decimals=1, at=(-57, 12))
asm.dimension(P("base", "base/side/e13"), P("base", "bore/side/e1"), centres=True, show_in="top", id="centre_y", decimals=1, at=(-19, -54))
asm.dimension(P("rack", "rack/edge[end+lip/e0/side/e0]"), P("rack", "rack/edge[bore/side/e1+lip/e0/side/e0]"), along=axis("Z"), show_in="right", id="chamfer", at=(40, 36))
asm.dimension(P("rack", ["rod_holes/side/e0", "rod_holes3/1/rod_holes/side/e0", "rod_holes3/2/rod_holes/side/e0"][int(os.environ.get("FI","1"))]), P("rack", "rack/end"), show_in="isoA", id="rod_below_top_s", decimals=3)
asm.dimension(P("base", "base/start"), P("base", fl[0]), show_in="isoA", id="floor_s", decimals=3)
asm.op("Section", id="A", along="Plane", plane=axis("X"), offset=0.75, flip=True)
asm.drawing(views=[view("front", "Front", (85, 90), style="HiddenDashed"),
                   view("top", "Top", (100, 215), style="HiddenDashed"),
                   projected("right", "front", "Right", 80, style="HiddenDashed"),
                   view("iso", "Isometric", (270, 235), scale=(1, 5)),
                   view("isoA", "Isometric", (360, 235), scale=(1, 5), section="A"),
                   view("isoBase", "Isometric", (270, 140), scale=(1, 5), bodies=["base"]),
                   view("isoRack", "IsometricBelowFrontRight", (355, 140), scale=(1, 5), bodies=["rack"])],
            scale=(1, 5), parts_list=["item", "qty", "name", "material", "density"], balloons=[balloon("base", "isoBase", (22, 16)), balloon("rack", "isoRack", (22, 16)), balloon("rod1", "iso", (24, 10))],
            notes=[note(None, (0, 0), "HOLE (6) PLACES", dimension="hole_dia"),
                   note(None, (0, 0), "(2) PLACES", dimension="bore_dia"),
                   note(None, (0, 0), "(2) PLACES", dimension="corner_r"),
                   note(None, (0, 0), "ROD (3) PLACES", dimension="rod_dia"),
                   note(None, (0, 0), "X 45° CHAMFER", dimension="chamfer"),
                   note("isoA", (0, -52), "SECTION VIEW"),
                   note(None, (0, 0), "RODS AND HOLES", dimension="rod_below_top_s"),
                   note(None, (0, 0), "RODS AND HOLES", dimension="floor_s")])
rb = asm.build(os.path.join(folder, "umbrella_stand.shapecraft"), render=True); rb.show()
print(rb.drawings[0]["problems"])
