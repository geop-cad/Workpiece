import sys, math, os, json
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import *
from workpiece import view, projected, section_view
here = os.path.dirname(os.path.abspath(__file__))
folder = os.path.join(here, "6.parts")
os.makedirs(folder, exist_ok=True)
material(folder, "Plain carbon steel", 7800)

def plate(D, T):
    """One standalone weight plate: revolved web profile (YZ: u = radius, v = axis), 5 spokes cut and patterned."""
    big = D == 210                      # the 210 carries the detailed views (as the original's main views)
    p = Part(unit="Millimetre", material="Plain carbon steel", file=f"plate{D}.workpiece", title=f"{D}mm PLATE")
    path = os.path.join(folder, p.file)
    R, h = D / 2, T / 2
    s = p.sketch("ZX", id="profile", show_in="secA_view" if big else None, on_drawing=big)
    pts = [(-h, 25), (h, 25), (h, 37.5), (h - 4, 41.5), (h - 4, R - 22), (h, R - 18), (h, R),
           (-h, R), (-h, R - 18), (-h + 4, R - 22), (-h + 4, 41.5), (-h, 37.5)]
    sh = s.polyline(pts, True, "pr")
    P, L = sh.points, sh.curves
    for a, b in [(0, 1), (11, 2), (10, 3), (9, 4), (8, 5), (7, 6)]:
        s.symmetric(P[a], P[b], "v")
    s.vertical(L[1]); s.vertical(L[3]); s.vertical(L[5])
    s.angle(L[3], L[2], 45, at=(-22, -17), id="a45_in"); s.angle(L[4], L[3], 45, at=(-22, -36), id="a45_out")
    s.diameter_about(P[1], "u", 50, on_drawing=False)
    if big:
        s.diameter_about(P[1], "u", 50, reference=True, show_in="secA_view", at=(22, 12), id="bore_ref")
    s.distance_u(P[0], P[1], T, on_drawing=False)
    s.diameter_about(P[2], "u", 75, on_drawing=False)
    s.distance_u(P[3], P[2], 4, at=(-8, -24))
    s.distance_u(P[4], P[5], 4, at=(-8, -42))
    s.distance_v(P[5], P[6], 18, at=(8, -48))
    s.diameter_about(P[6], "u", D, on_drawing=False)
    p.op("Revolve", "plate", id="plate", sketch=profile(s), axis="SketchU", combine="NewBody")
    r = p.build(path)
    top_ring = lambda rad: r.edges(where=lambda e: e["name"].startswith("plate/ring") and abs(e["from"][2] - h) < 0.01
                                   and abs(math.hypot(e["from"][0], e["from"][1]) - rad) < 0.01 and math.dist(e["from"], e["to"]) < 1e-6)[0]
    hub_edge, rim_edge = top_ring(37.5), top_ring(R - 18)

    q = p.sketch("XY", id="pocket", show_in="top" if big else None, on_drawing=big)
    q.project("hub", hub_edge)
    q.project("rim", rim_edge)
    C = q.point(-65.5, 0); L0 = q.point(-55.5, -17.3); R0 = q.point(-55.5, 17.3)
    TL = q.point(-77.4, -30); TR = q.point(-77.4, 30)
    B = q.point(-45.5, 0); H = q.point(-37.5, 0)
    arcA = q.arc(C, L0, R0)
    left = q.line(L0, TL); right = q.line(R0, TR)
    arcB = q.arc(q.origin, TR, TL)
    q.point_on(C, "u"); q.equal(left, right)
    q.radius(arcA, 20, at=(-52, 45), id="r20"); q.tangent(left, arcA); q.tangent(right, arcA)
    q.angle(right, left, 60, at=(-30, 56), id="a60")
    q.point_on(TR, "rim")
    q.point_on(B, arcA); q.point_on(B, "u")
    q.point_on(H, "hub"); q.point_on(H, "u")
    q.distance_u(B, H, 8, at=(62, 22), id="g8")
    p.cut(q, through=True, symmetric=True, target=topo("plate"), id="pocket_cut")
    r = p.build(path)
    b = next(b for b in r.bodies if b["name"] == "plate")
    wall = [f["name"] for f in b["faces"] if f["name"].startswith("pocket_cut") and f["middle"][0] < -50]
    p.feature_pattern(wall, 5, about=axis("Z"), id="spokes")
    r = p.build(path)
    b = next(b for b in r.bodies if b["name"] == "plate")
    fl = [f for f in b["faces"] if f["name"].startswith("plate")]
    top = max((f for f in fl if f["flat"] and f["middle"][2] > T / 2 - 0.01), key=lambda f: f["area"])
    bot = max((f for f in fl if f["flat"] and f["middle"][2] < -T / 2 + 0.01), key=lambda f: f["area"])
    out = max((f for f in fl if not f["flat"]), key=lambda f: f["area"])
    p.dimension(top["name"], bot["name"], show_in="front", **({"at": (34, 0)} if not big else {"at": (-62, 0)}), id="thick")
    p.dimension(out["name"], show_in="front", kind="Diameter", id="dia")
    if big:
        bore = min((f for f in fl if not f["flat"]), key=lambda f: math.hypot(f["middle"][0], f["middle"][1]))
        p.dimension(bore["name"], show_in="top", kind="Diameter", at=(48, 55), id="bore")
        p.dimension(hub_edge, show_in="top", kind="Diameter", at=(35, -45), id="hubdia")
        p.op("Section", "secA", plane=axis("Y"))
        p.drawing(scale=(1, 2), format="A3",
                  views=[view("top", "Top", (85, 200)), view("front", "Front", (110, 85), style="HiddenDashed"),
                         section_view("secA_view", "secA", (215, 175), of="top", turn=90, half="Below", half_only=True),
                         view("iso", "Isometric", (330, 120), scale=(1, 5), style="Shaded")],
                  notes=[note(None, (0, 0), "TYP", dimension="pocket/r20"), note(None, (0, 0), "TYP", dimension="pocket/a60"),
                         note(None, (0, 0), "TYP", dimension="pocket/g8"), note(None, (0, 0), "THRU", dimension="profile/bore_ref"),
                         note(None, (0, 0), "THRU", dimension="bore")])
    else:
        p.drawing(scale=(1, 2), views=[view("front", "Front", (100, 145), style="HiddenDashed")])
    hubtop = max((f for f in fl if f["flat"] and abs(f["middle"][2] - h) < 0.01 and math.hypot(f["middle"][0], f["middle"][1]) < 30), key=lambda f: f["area"])
    floor = max((f for f in fl if f["flat"] and abs(f["middle"][2] - (h - 4)) < 0.01), key=lambda f: f["area"])
    cone_in = max((f for f in fl if not f["flat"] and abs(math.hypot(f["middle"][0], f["middle"][1]) - 39.5) < 1 and f["middle"][2] > 0), key=lambda f: f["area"])
    cone_out = min((f for f in fl if not f["flat"] and abs(math.hypot(f["middle"][0], f["middle"][1]) - 85) < 3 and f["middle"][2] > 0 and f["middle"][0] > 0
                    and abs(f["middle"][1]) < 15), key=lambda f: abs(f["middle"][1])) if D == 210 else None
    pk = {f["name"]: f for f in b["faces"] if f["name"].startswith("pocket_cut/side/")}
    p.n = {"top": top["name"], "bot": bot["name"], "out": out["name"], "hubtop": hubtop["name"], "floor": floor["name"],
           "rim_out": top_ring(R), "cone_in": cone_in["name"], "cone_out": cone_out["name"] if cone_out else None, "hub_edge": hub_edge}
    if big:
        p.n["bore"] = bore["name"]
        p.n["pocket_r"] = next(n for n, f in pk.items() if not f["flat"] and f["middle"][0] > -50)
        p.n["pocket_w"] = [n for n, f in pk.items() if f["flat"]]
    p.names = (top["name"], bot["name"], out["name"])
    return p

plates = {D: plate(D, T) for D, T in [(210, 18), (270, 30), (330, 48)]}
for D, p in plates.items():
    r = p.build(os.path.join(folder, p.file), render=True); r.show()
    print(D, "mass kg", r.mass(7800e-9))

# The assembly: the three plates stacked on the axis, 200 apart, as the drawing's edge views stand one over the other.
asm = Part()
Z = {210: 400, 270: 200, 330: 0}
for D in (210, 270, 330):
    asm.add_part(plates[D], id=f"plate{D}", at=(0, 0, Z[D]))
r = asm.build(os.path.join(folder, "plates.workpiece"))
r.show()
# The 210's face view and section A-A carry the drawing's dimensions of the plate (measured on the placed plate).
n = plates[210].n
pn = "plate210"
P = lambda name: placed(pn, topo(name))
asm.dimension(P(n["pocket_r"]), kind="Radius", show_in="face210", at=(-56, 36), id="r20")
asm.dimension(P(n["pocket_w"][0]), P(n["pocket_w"][1]), kind="Angle", supplement=False, show_in="face210", at=(-22, 60), id="a60")
asm.dimension(P(n["pocket_r"]), P(n["hub_edge"]), show_in="face210", at=(62, 22), id="g8")
asm.dimension(P(n["bore"]), kind="Diameter", show_in="face210", at=(48, 55), id="bore")
asm.dimension(P(n["hub_edge"]), kind="Diameter", show_in="face210", at=(35, -45), id="hubdia")
asm.dimension(P(n["bore"]), kind="Diameter", reference=True, show_in="secA_view", at=(22, 12), id="bore_ref")
asm.dimension(P(n["hubtop"]), P(n["floor"]), show_in="secA_view", at=(-8, -24), id="d4_in")
asm.dimension(P(n["floor"]), P(n["top"]), show_in="secA_view", at=(-8, -42), id="d4_out")
asm.dimension(P(n["top"]), P(n["bot"]), show_in="secA_view", at=(16, -40), id="t18_sec")
# The edge views: thickness and diameter of each (the 210's twice: large, and in the column with the others).
asm.dimension(P(plates[210].names[2]), kind="Diameter", show_in="edge210", id="d210_big")
asm.dimension(P(plates[210].names[0]), P(plates[210].names[1]), show_in="edge210", id="t210_big", at=(-68, 0))
for D in (210, 270, 330):
    q = f"plate{D}"
    top = plates[D].names
    row = {210: 40, 270: 0, 330: -40}[D]
    asm.dimension(placed(q, topo(top[0])), placed(q, topo(top[1])), show_in="front", id=f"t{D}", at=(38, row - 6))
    asm.dimension(placed(q, topo(top[2])), kind="Diameter", show_in="front", id=f"d{D}", at=(-20, row - 14))
cut = asm.sketch("XY", id="cutline", on_drawing=False)
cut.project("rim", placed(pn, topo(n["rim_out"])))   # the 210's rim (top edge, Ø210), seen from above
E0 = cut.point(0, 0); E1 = cut.point(100, 0)
cl = cut.line(E0, E1)
cut.point_on(E0, "v"); cut.point_on(E0, "u")
cut.point_on(E1, "u"); cut.point_on(E1, "rim")   # the line ends on the rim: no 105 typed
asm.op("Section", "secA", along="Sketch", sketch=profile(cut))
asm.on_sheet(placed("plate210", "profile/a45_in"), show_in="secA_view", at=(-22, -17))
asm.on_sheet(placed("plate210", "profile/a45_out"), show_in="secA_view", at=(-22, -36))
H = "HiddenDashed"
asm.drawing(scale=(1, 2), format="A3", views=[
    view("face210", "Top", (88, 218), bodies=["plate210"]),
    section_view("secA_view", "secA", (205, 190), of="face210", turn=90, half="Below", half_only=True, bodies=["plate210"]),
    view("edge210", "Front", (100, 62), bodies=["plate210"], style=H),
    view("front", "Front", (362, 172), scale=(1, 5), style=H),
    view("iso210", "Isometric", (262, 245), scale=(1, 5), style="Shaded", bodies=["plate210"]),
    view("iso270", "Isometric", (262, 175), scale=(1, 5), style="Shaded", bodies=["plate270"]),
    view("iso330", "Isometric", (262, 100), scale=(1, 5), style="Shaded", bodies=["plate330"])],
    notes=[note("iso210", (0, 17), "210mm PLATE"), note("iso270", (0, 21), "270mm PLATE"), note("iso330", (0, 30), "330mm PLATE"),
           note("edge210", (50, -38), "THESE ARE THE ONLY DIMENSIONS THAT CHANGE", to=[(-105, -9), (60, -9)]),
           note(None, (0, 0), "TYP", dimension="r20"), note(None, (0, 0), "TYP", dimension="a60"), note(None, (0, 0), "TYP", dimension="g8"),
           note(None, (0, 0), "THRU", dimension="bore"), note(None, (0, 0), "THRU", dimension="bore_ref")])
r = asm.build(os.path.join(folder, "plates.workpiece"), render=True)
r.show()
print(r.drawings[0]["problems"])
for m in r.masses:
    print(m["name"], m["file"], m["material"], round(m["mass"], 4), "kg")
print("total", r.mass_total, "kg (expected 25.31 +- 0.02)")
