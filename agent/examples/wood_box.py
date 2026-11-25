"""Wooden box with two metal dividers (an assembly of two parts, mm): an example of a multi-part script with
a declared original. Run it, then read out/box.parts/box_assembly.shapecraft.views/sheet.txt and the scorecard."""
import math, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "python"))  # a checkout; `pip install shapecraft-cad` makes this line unnecessary
from shapecraft import Part, axis, topo, datum, profile, view, projected, detail_view, section_view, note, material, anchor, placed, balloon, original
here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")
folder = os.path.join(here, "box.parts")
os.makedirs(folder, exist_ok=True)
material(folder, "American cherry", 570)
material(folder, "6061 aluminium", 2700)
out = os.path.join(folder, "box.shapecraft")
p = Part(material="American cherry", file="box.shapecraft", title="WOOD BOX")

# The wooden box, long side along y (so the front view shows it as the drawing does): 200 x 100 x 92, its rim on
# the origin plane z = 0, hanging down (so "18 from the top" and "4 below the rim" are measured from the origin).
s = p.sketch("XY", id="plan", show_in="top")
r = s.rect("r", -50, -100, 100, 200)
s.symmetric(r.points[0], r.points[1], "v")
s.symmetric(r.points[1], r.points[2], "u")
s.distance_u(r.points[0], r.points[1], 100)
s.distance_v(r.points[1], r.points[2], 200)
p.extrude(s, 92, combine="NewBody", flip=True, id="box")
rep = p.build(out)
vert = rep.edges(where=lambda e: e["straight"] and abs(e["from"][0]-e["to"][0])<1e-6 and abs(e["from"][1]-e["to"][1])<1e-6)
p.fillet(vert, 32, id="corners")
rep = p.build(out)
bot = rep.edges(where=lambda e: abs(e["from"][2]+92)<1e-6 and abs(e["to"][2]+92)<1e-6)
p.fillet(bot, 6, id="bottom")
rep = p.build(out)
top = rep.faces(where=lambda f: f["flat"] and abs(f["normal"][2]-1)<1e-6 and abs(f["middle"][2])<1e-6)
p.shell(top, 9, id="wall")
rep = p.build(out)
rim = rep.edges(where=lambda e: abs(e["middle"][2])<1e-3 and "thick" not in e["name"])
p.fillet(rim, 3, id="rim")

# Handle slots: 60 x 20 obround, 18 below the rim, through both end walls (u = z, v = x; long along v).
h = p.sketch("ZX", id="slot_outline", show_in="right")
c0 = h.point(-28, -20, "c0"); c1 = h.point(-28, 20, "c1")
a0 = h.point(-18, -20, "a0"); b0 = h.point(-38, -20, "b0")
a1 = h.point(-18, 20, "a1"); b1 = h.point(-38, 20, "b1")
L = h.point(-28, -30, "L"); R = h.point(-28, 30, "R")
la = h.line(a0, a1, "la"); lb = h.line(b0, b1, "lb")
arc0 = h.arc(c0, b0, a0, "arc0"); arc1 = h.arc(c1, a1, b1, "arc1")
h.tangent(la, arc0); h.tangent(la, arc1); h.tangent(lb, arc0); h.tangent(lb, arc1)
h.equal(arc0, arc1)
h.symmetric(c0, c1, "u")
h.point_on(L, arc0); h.point_on(R, arc1); h.vertical(c0, L); h.vertical(c1, R)
h.distance_u(b0, a0, 20)
h.distance_v(L, R, 60)
h.point_line(h.origin, la, 18)
p.cut(h, through=True, symmetric=True, id="slots")
rep = p.build(out)
floor = rep.faces(where=lambda f: f["flat"] and abs(f["normal"][2]-1)<1e-6 and abs(f["middle"][2]+83)<1e-6)[0]

# Grooves for the dividers in the walls: 0.7 clearance around a divider's footprint (88 x 4, inner faces 60 apart: a
# construction rectangle, a construction point mirrors its inner face about u), from the floor up through the walls only.
g = p.sketch(topo(floor), id="groove", on_drawing=False)

dr = g.rect("d", -44, 30, 88, 4)
g.construction(*dr.curves)
g.symmetric(dr.points[0], dr.points[1], "v")
g.distance_u(dr.points[0], dr.points[1], 88, on_drawing=False)
g.distance_v(dr.points[0], dr.points[3], 4, on_drawing=False)
m = g.point(-44, -30, "m")
g.symmetric(dr.points[0], m, "u")
g.distance_v(m, dr.points[0], 60)
gr = g.rect("g", -44.7, 29.3, 89.4, 5.4)
g.symmetric(gr.points[0], gr.points[1], "v")
g.distance_v(gr.points[0], dr.points[0], 0.7)
g.distance_v(dr.points[3], gr.points[3], 0.7)
g.distance_u(gr.points[0], dr.points[0], 0.7)
p.cut(g, through=True, target=topo("box"), id="groove_cut")
rep = p.build(out)
end = rep.faces(where=lambda f: f["name"].startswith("groove_cut/") and f["flat"] and abs(f["normal"][0]+1)<1e-6)[0]
p.feature_mirror([end], "Y", id="groove2")
p.on_sheet("rim", on_drawing=False)
p.on_sheet("bottom", on_drawing=False)
p.on_sheet("groove2", on_drawing=False)
p.dimension("rim/e1/side/e0", show_in="right", id="rim_right")
p.dimension("bottom/e1/side/e0", show_in="right", id="bottom_right")
p.on_sheet("wall", on_drawing=False)
p.dimension("rim/e0/side/e0", show_in="front", reference=True, id="rim_ref")
p.dimension("bottom/e0/side/e0", show_in="front", reference=True, id="bottom_ref")
p.dimension("box/side/e12", "thick/f1/far#2#2", show_in="top", reference=True, id="wall_top", at=(-70, -22))
p.dimension("box/end", "thick/f4/far", show_in="front", reference=True, id="floor_front")
p.dimension("box/end", "thick/f4/far", show_in="right", reference=True, id="floor_right")
# (The inner round of the corner the R32 stands at, upper right in the top view.)
inner = rep.faces(where=lambda f: f["name"].startswith("thick/") and not f["flat"] and f["middle"][0] < 0 and f["middle"][1] > 0 and f["middle"][2] > -80)[0]
p.dimension(inner, show_in="top", reference=True, kind="Radius", id="inner_r", at=(42, 42))
p.on_sheet("corners", show_in="top", at=(70, 30))
gf = lambda n: rep.faces(where=lambda f: f["name"].startswith("groove_cut/") and f["flat"] and abs(f["normal"][1]-n)<1e-6 and f["middle"][0] < 0 and f["middle"][1] > 0)[0]
gw = [gf(1), gf(-1)]
endf = rep.faces(where=lambda f: f["flat"] and abs(f["normal"][1]-1)<1e-6 and abs(f["middle"][1]-100)<1e-6)[0]
# (The divider's footprint, its 60 and the 0.7 clearances are the assembly's: its detail shows a divider in its groove.)
p.drawing(views=[view("top", "Top", (190, 215)), projected("front", "top", "Below", 80, style="HiddenDashed"),
                 projected("right", "front", "Right", 120, style="HiddenDashed")], scale=(1, 2), comma=False,
         notes=[note(None, (0, 0), "(4) PLACES", dimension="corners")])
rep = p.build(out, render=True); rep.show()

# The divider, modelled where it stands in the box (origin = the box's rim middle): its footprint 4 thick with its inner
# face 60 from the other divider's (a construction point mirrors it about v), its top 4 below the rim, its foot 9 above
# the box bottom (a construction point 92 below the rim), 88 wide. The second divider is this one turned half a turn.
dv = Part(material="6061 aluminium", file="divider.shapecraft", title="METAL DIVIDERS")
d = dv.sketch("YZ", id="divider_outline", show_in="front")
r = d.rect("d", 30, -83, 4, 79)
m = d.point(-30, -4, "m")
bt = d.point(0, -92, "bt")
d.symmetric(r.points[3], m, "v")
d.distance_u(m, r.points[3], 60, id="between", on_drawing=False)
d.distance_u(r.points[3], r.points[2], 4, id="thick")
d.distance_v(r.points[3], d.origin, 4, id="below_rim", on_drawing=False)
d.point_on(bt, "v")
d.distance_v(bt, d.origin, 92, id="height", on_drawing=False)
d.distance_v(bt, r.points[0], 9, id="above_bottom", on_drawing=False)
dv.extrude(d, 88, symmetric=True, combine="NewBody", id="divider")
dv.drawing(views=[view("front", "Front", (110, 150)), projected("right", "front", "Right", 90),
                  view("iso", "Isometric", (280, 150))], scale=(1, 1), comma=False)
drep = dv.build(os.path.join(folder, "divider.shapecraft"), render=True); drep.show()
# Assembly: the box and the divider, both modelled where they stand; the second divider is the first turned half a turn.
asm = Part()
# What the original sheet has, read once from its picture (mass question: XXX.X g +/- 0.3).
asm.expect(original(
    views=[("top", "Top"), ("front", "Front"), ("right", "Right"), ("detail", "Detail"),
           ("iso", "Pictorial"), ("iso2", "Pictorial"), ("iso3", "Pictorial")],
    numbers=[.7, .7, .7, "(R23)", "R32", 60, 100, "(9)", 200, 88, 4, "(R3)", 4, 18, "(4)", 92, "R3", "R6", 20, 60, 9,
             "(9)", "(9)", "(9)", "(R6)", "(4)"],
    notes=["(4) PLACES", "WIDE", "THICK", "WALL TYP", "OUTER TOP EDGE", "OUTER BOTTOM EDGE", "(2) PLACES", "(2) PLACES",
           "(2) PLACES", "CL SYM", "TO TOP OF DIVIDER", "TO BOTTOM OF DIVIDER", "WALL THK TYP"],
    mass=(450.0, 0.3, "g"), units="mm", decimal="."))
asm.add_part(p, id="box")
asm.add_part(dv, id="divider1")
asm.add_part(dv, id="divider2", euler=(0, 0, 180))
rb = asm.build(os.path.join(folder, "box_assembly.shapecraft")); rb.show()
for pp in rb.placed: print(pp["name"], pp["pose"]["at"], pp["pose"]["euler"])
for m in rb.masses: print(m["name"], m["material"], m["volume"], m["mass"] * 1000, "g")
print("total g", rb.mass_total * 1000, "expected 450.0 +- 0.3")

# The assembly's sheet: the box with its dividers; every number of the original, measured on the placed parts.
P = lambda inst, name: placed(inst, topo(name))
def flat(inst, n, k, val):
    return rb.faces(placed=inst, where=lambda f: f["flat"] and abs(f["normal"][k] - n) < 1e-6 and abs(f["middle"][k] - val) < 1e-6 and f["area"] > 100)[0]
asm.dimension(P("box", flat("box", 1, 1, 100)), P("box", flat("box", -1, 1, -100)), show_in="top", id="length", at=(-30, -45))
asm.dimension(P("box", flat("box", 1, 0, 50)), P("box", flat("box", -1, 0, -50)), show_in="top", id="width")
asm.dimension(P("box", "box/start"), P("box", "box/end"), show_in="front", id="height", at=(60, 0))
asm.dimension(P("divider1", flat("divider1", 1, 0, 44)), P("divider1", flat("divider1", -1, 0, -44)), show_in="top", id="div_wide", at=(27, 14))
asm.dimension(P("divider1", flat("divider1", 1, 1, 34)), P("divider1", flat("divider1", -1, 1, 30)), show_in="top", id="div_thick", at=(30, -34))
asm.dimension(P("divider1", flat("divider1", 1, 0, 44)), P("divider1", flat("divider1", -1, 0, -44)), show_in="iso3", id="div_wide_p")
asm.dimension(P("divider1", flat("divider1", 1, 1, 34)), P("divider1", flat("divider1", -1, 1, 30)), show_in="iso3", id="div_thick_p")
dtop = rb.faces(placed="divider1", where=lambda f: f["flat"] and abs(f["normal"][2] - 1) < 1e-6)[0]
dfoot = rb.faces(placed="divider1", where=lambda f: f["flat"] and abs(f["normal"][2] + 1) < 1e-6)[0]
asm.dimension(P("box", "box/start"), P("divider1", dtop), show_in="front", id="below_rim", at=(-7, 12))
asm.dimension(P("box", "box/end"), P("divider1", dfoot), show_in="front", id="above_bottom", at=(-7, -14))
asm.dimension(P("divider1", flat("divider1", -1, 1, 30)), P("divider2", flat("divider2", 1, 1, -30)), show_in="top", id="between", at=(0, 9))
# The box's own numbers: corner radii, wall, floor, edge rounds, handle slot.
corner = rb.faces(placed="box", where=lambda f: f["name"].startswith("corners/") and not f["flat"] and f["middle"][0] < 0 and f["middle"][1] > 0)[0]
asm.dimension(P("box", corner), show_in="top", kind="Radius", id="r_corner", at=(70, 30))
asm.dimension(P("box", inner), show_in="top", kind="Radius", reference=True, id="r_inner", at=(42, 42))
asm.dimension(P("box", "box/side/e12"), P("box", "thick/f1/far#2#2"), show_in="top", reference=True, id="wall_top", at=(-70, -22))
asm.dimension(P("box", "box/end"), P("box", "thick/f4/far"), show_in="front", reference=True, id="floor_front")
asm.dimension(P("box", "box/end"), P("box", "thick/f4/far"), show_in="right", reference=True, id="floor_right")
asm.dimension(P("box", "rim/e0/side/e0"), show_in="front", reference=True, kind="Radius", id="rim_front")
asm.dimension(P("box", "bottom/e0/side/e0"), show_in="front", kind="Radius", id="bottom_front")
asm.dimension(P("box", "rim/e1/side/e0"), show_in="right", kind="Radius", id="rim_right", at=(-30, 44))
asm.dimension(P("box", "bottom/e1/side/e0"), show_in="right", kind="Radius", id="bottom_right", at=(-26, -46))
asm.dimension(P("box", "box/start"), P("box", "slots/side/e15"), show_in="right", id="slot_depth", at=(-34, 18))
asm.dimension(P("box", "slots/side/e15"), P("box", "slots/side/e16"), show_in="right", id="slot_width", at=(-46, -9))
asm.dimension(P("box", "slots/side/e17"), P("box", "slots/side/e18"), show_in="right", far=True, id="slot_length", at=(8, -32))
# The detail of a divider in its groove: the clearances, the box's groove faces against the divider's.
groove = lambda k, n, val: placed("box", topo(rb.faces(placed="box", where=lambda f: f["flat"] and f["name"].startswith("groove") and abs(f["normal"][k] - n) < 1e-6 and abs(f["middle"][k] - val) < 1e-6 and f["middle"][0] < 0)[0]))
asm.dimension(groove(1, 1, -34.7), P("divider2", flat("divider2", -1, 1, -34)), show_in="detail", id="clear_a", at=(24, 20))
asm.dimension(groove(1, -1, -29.3), P("divider2", flat("divider2", 1, 1, -30)), show_in="detail", id="clear_b", at=(26, 39))
asm.dimension(groove(0, 1, -44.7), P("divider2", flat("divider2", -1, 0, -44)), show_in="detail", id="clear_end", at=(-24, -30))
pY = asm.plane("Y", 0, id="midY"); pX = asm.plane("X", 0, id="midX")
asm.op("Section", id="A", along="Plane", plane=axis("X"), offset=0.0)
asm.drawing(views=[view("top", "Top", (190, 215)), projected("front", "top", "Below", 80, style="HiddenDashed"),
                   projected("right", "front", "Right", 120, style="HiddenDashed"), view("iso", "Isometric", (342, 236), scale=(1, 2)),
                   view("iso2", "IsometricBackLeft", (372, 112), scale=(1, 5)),
                   view("iso3", "Isometric", (60, 150), scale=(1, 2), bodies=["divider1"]),
                   detail_view("detail", "top", (-32, 44), 4, (75, 225), scale=(10, 1)),
                   section_view("secA", "A", (85, 60), of="top")],
            scale=(1, 2), comma=False, parts_list=["item", "qty", "name", "material", "density"], centre_lines=[{"of": pY, "view": "top", "label": "CL SYM", "symmetry": True}, {"of": pY, "view": "front", "label": "CL SYM", "symmetry": True}, {"of": pX, "view": "right", "label": "CL SYM", "symmetry": True}], balloons=[balloon("box", "iso", (-52, 34)), balloon("divider1", "iso", (6, 44))],
            notes=[note(None, (0, 0), "WIDE", dimension="div_wide"), note(None, (0, 0), "THICK", dimension="div_thick"),
                   note(None, (0, 0), "(4) PLACES", dimension="r_corner"), note(None, (0, 0), "WALL TYP", dimension="wall_top"), note(None, (0, 0), "OUTER\nTOP EDGE", dimension="rim_right"), note(None, (0, 0), "OUTER\nBOTTOM EDGE", dimension="bottom_right"), note(None, (0, 0), "WIDE", dimension="div_wide_p"), note(None, (0, 0), "THICK", dimension="div_thick_p"),
                   note(None, (0, 0), "(2) PLACES", dimension="slot_width"), note(None, (0, -3), "(2) PLACES", dimension="slot_length")])
rb = asm.build(os.path.join(folder, "box_assembly.shapecraft"), render=True); rb.show()
print(rb.drawings[0]["problems"])
