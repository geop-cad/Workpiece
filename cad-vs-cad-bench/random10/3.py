import math, os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import Part, axis, topo, datum, profile, view, projected, section_view, material, note
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "3.workpiece")
material(os.path.dirname(out), "1060 Alloy", 2700)
p = Part(material="1060 Alloy")
DENS = 2.7e-3   # g/mm3

# Origin: centre of the clamp ring at the bottom of the part. Ring axis = Z. The elbow lies in the YZ plane (the
# drawing's front view: leg on the left, ring on the right); the clamp boss axis runs along X at y = 28, z = 20.
# --- the arm: an I section swept along the elbow's outer face (leg up at y = -70, R40 bend, then along the underside z = 5 to the ring).
# The section stands on the plane y = 0 where the run meets the ring, so no plane of its own is needed.
pa = p.sketch("YZ", id="path", show_in="front")   # u = y, v = z
S = pa.point(-70, 105); T = pa.point(-70, 45); U = pa.point(-30, 5); W = pa.point(0, 5)
c = pa.point(-30, 45)
lST = pa.line(S, T); lUW = pa.line(U, W)
arc = pa.arc(c, U, T, cw=True)
pa.vertical(lST); pa.horizontal(lUW)
pa.tangent(arc, lST); pa.tangent(arc, lUW)
pa.radius(arc, 40, at=(-44, -44))
pa.point_on(W, "v")
pa.distance_v(pa.origin, W, 5, at=(-52, -50))
pa.distance_u(pa.origin, S, 70, at=(-35, -50))
pa.distance_v(U, S, 100, at=(-52, 15))

# The I section (u = z, v = x): 30 deep, 30 wide, 6 web, flange faces at 8 degrees to a level line, 16 degrees between the
# two faces of a flange so each flange thins to 4 at its tip.
q = p.sketch("ZX", id="section", show_in="section_a")
pts = {
 "A": (5, 0), "B": (7.1, -15), "C": (11.1, -15), "D": (12.8, -3), "E": (27.2, -3), "F": (28.9, -15), "G": (32.9, -15), "H": (35, 0),
 "G2": (32.9, 15), "F2": (28.9, 15), "E2": (27.2, 3), "D2": (12.8, 3), "C2": (11.1, 15), "B2": (7.1, 15)}
P = {k: q.point(*v, name=k) for k, v in pts.items()}
order = ["A","B","C","D","E","F","G","H","G2","F2","E2","D2","C2","B2"]
L = {}
for i, k in enumerate(order):
    n = order[(i+1) % len(order)]
    L[k+n] = q.line(n, k, name="l_"+k+n) if k in ("A", "E") else q.line(k, n, name="l_"+k+n)
q.point_on("A", "u"); q.point_on("H", "u")
for a_, b_ in (("B","B2"),("C","C2"),("D","D2"),("E","E2"),("F","F2"),("G","G2")):
    q.symmetric(a_, b_, "u")
q.horizontal(L["BC"]); q.horizontal(L["DE"]); q.horizontal(L["FG"])
q.distance_u(q.origin, "A", 5, on_drawing=False)
q.distance_u("A", "H", 30)
q.distance_v("B", "B2", 30)
q.distance_v("G", "G2", 30, on_drawing=False)
X = q.point(5, -15, "X"); Y = q.point(35, -15, "Y")
q.point_on(X, L["BC"]); q.point_on(Y, L["FG"])
XA = q.line(X, "A", construction=True); YH = q.line(Y, "H", construction=True)
q.vertical(XA); q.vertical(YH)
q.angle(XA, L["AB"], 8)
q.angle(L["GH"], YH, 8)
q.angle(L["CD"], L["AB"], 16, on_drawing=False)
q.angle(L["GH"], L["EF"], 16, on_drawing=False)
q.distance_u("B", "C", 4)
q.distance_u("F", "G", 4)
q.distance_v("D", "D2", 6)
p.op("SweptLoft", id="arm", sections=[{"of": {"Profile": "section"}}], rails=[{"of": {"Profile": "path"}, "role": "Path"}], combine="NewBody")
# Tube at the top of the leg: axis along Y, 100 above the arm's underside, 30 long, 38, with a 24 hole through it.
yminus70 = p.plane("Y", -70, id="y70")
p.on_sheet("y70", on_drawing=False)
tb = p.sketch(datum(yminus70), id="tube", show_in="right")   # u = z, v = x
tc = tb.dot(105, 0)
tb.project("top", "arm/run2/end/e0")   # the leg ends at the tube's axis
tb.point_on(tc, "top"); tb.point_on(tc, "u")
o = tb.circle(tc, 19)
tb.diameter(o, 38)
p.extrude(tb, 30, id="tube_ext")
p.hole(tb, 24, id="tube_hole")
p.on_sheet("tube_hole", show_in="right", at=(42, 22))
p.on_sheet("tube_ext", on_drawing=False)

# Clamp ring: dia 45 about Z, 40 high; bore dia 32 through it.
rg = p.sketch("XY", id="ring", show_in="top")
rc = rg.circle(rg.origin, 22.5)
rg.diameter(rc, 45, at=(-28, 38))
p.extrude(rg, 40, id="ring_ext")

# Clamp boss: 60 long along X, its axis 28 off the ring's and 20 up (the planes y = 28 and z = 20 meet in it), dia 26 at its
# ends, swelling to the middle (section C-C: 8 degrees between the two sides of each half). Drawn on the plane y = 28 (u = z, v = x).
y28 = p.plane("Y", 28, id="y28")
z20 = p.plane("Z", 20, id="z20")
p.op("AddAxis", id="boss_axis", mode="PlanePlane", references=[datum(y28), datum(z20)])
p.on_sheet("y28", on_drawing=False)
p.on_sheet("z20", on_drawing=False)
bs = p.sketch(datum(y28), id="boss", show_in="section_c")
bs.project("zl", datum(z20))
P1 = bs.point(20, 30); P2 = bs.point(33, 30); P3 = bs.point(35, 0); P4 = bs.point(33, -30); P5 = bs.point(20, -30)
la = bs.line(P1, P5); l12 = bs.line(P1, P2); l23 = bs.line(P2, P3); l34 = bs.line(P3, P4); l45 = bs.line(P4, P5)
bs.horizontal(l12)
bs.point_on(P3, "u")
bs.symmetric(P1, P5, "u"); bs.horizontal(l45)
bs.collinear(la, "zl")
bs.distance_v(P5, P1, 60, show_in="right")
bs.diameter_about(P2, la, 26, show_in="right", at=(34, 0))
m2 = bs.point(7, 30, "m2"); m3 = bs.point(5, 0, "m3"); m4 = bs.point(7, -30, "m4")
bs.symmetric(P2, m2, la); bs.symmetric(P3, m3, la); bs.symmetric(P4, m4, la)
lm = bs.line(m2, m3, construction=True); lm2 = bs.line(m3, m4, construction=True)
bs.angle(lm, l23, 8, at=(-30, -32))
bs.angle(l34, lm2, 8, at=(31, -32))
p.op("Revolve", id="boss_rev", sketch={"Profile": "boss"}, axis=datum("boss_axis"), angle=360.0, combine="Join")

bo = p.sketch("XY", id="bore", on_drawing=False)
bc = bo.circle(bo.origin, 16)
bo.diameter(bc, 32)
p.cut(bo, through=True, id="bore_cut")

# Clamp slit, 4 wide about the YZ plane, from the bore out through the boss, through everything in z.
sl = p.sketch("XY", id="slit", show_in="top")
sr = sl.rect("s", -2, 0, 4, 50)
sl.symmetric(sr.points[0], sr.points[1], "v")
sl.coincident(sr.points[0], sl.origin) if False else sl.point_on(sr.points[0], "u")
sl.distance_u(sr.points[0], sr.points[1], 4)
sl.distance_v(sr.points[1], sr.points[2], 50, on_drawing=False)
p.cut(sl, through=True, symmetric=True, id="slit_cut")
rep = p.build(out); rep.show()

# Bolt hole dia 11 through the boss: dia 20 counterbore 4 deep at the +X end (hole at a dot of a sketch on that end face);
# a 15 across-flats hex pocket 4 deep at the -X end.
endp = rep.faces(where=lambda f: f["name"].startswith("boss_rev/") and f["flat"] and f["middle"][0] > 29)[0]
endm = rep.faces(where=lambda f: f["name"].startswith("boss_rev/") and f["flat"] and f["middle"][0] < -29)[0]
rim = lambda x: rep.edges(where=lambda e: abs(e["middle"][0] - x) < 1e-6 and not e["straight"] and e["length"] > 20)[0]
hs = p.sketch(topo(endp), id="bolt", show_in="right")
hd = hs.dot(28, 20)
hs.project("rim", rim(30)); hs.coincident(hd, "rim_center")
p.hole(hs, 11, kind="Counterbore", head=20, head_depth=4, id="bolt_hole")
hx = p.sketch(topo(endm), id="hex_pocket", on_drawing=False)
hc = hx.point(28, -20, "hc")
hx.project("rim", rim(-30)); hx.coincident(hc, "rim_center")
hex_ = hx.polygon("hex", hc, 6, 15, guess=(28, -20))
hx.horizontal(hex_.curves[0])
p.cut(hx, 4, id="hex_cut")
rep = p.build(out); rep.show()


# Numbers no sketch carries: the bore, the hex across flats, the counterbore's depth and diameter, the radii, the halves.
end30 = "boss_rev/side/e22"
tube_cyl = rep.faces(where=lambda f: f["name"].startswith("tube_ext/side/") and not f["flat"])[0]
endrim = rep.edges(where=lambda e: abs(e["middle"][0] - 30) < 1e-6 and not e["straight"] and e["length"] > 70)
print("end rims", endrim)
p.dimension("bore_cut/side/e7", kind="Diameter", show_in="top", id="bore_dia", at=(-5, -27))
p.dimension("hex_cut/side/e16", "hex_cut/side/e19", show_in="front", id="hex_af", at=(56, -64))
p.dimension(tube_cyl, kind="Diameter", show_in="top", id="tube_dia_top", at=(-54, 6))
p.on_sheet("bolt_hole", on_drawing=False)
x0 = p.plane("X", 0, id="x0")
p.on_sheet("x0", on_drawing=False)
p.dimension("bolt_hole/side/e3", kind="Diameter", show_in="front", id="hole_dia_front", at=(46, 6))
p.dimension("bolt_hole/side/e3", kind="Diameter", show_in="top", reference=True, id="hole_dia_ref", at=(42, 36))
p.dimension("bolt_hole/side/e1", kind="Diameter", show_in="right", id="cbore_dia", at=(46, -6))
p.dimension(end30, "bolt_hole/side/e2", show_in="section_c", id="cbore_depth", at=(-30, 20))
p.dimension(endrim[0], show_in="front", id="end_dia", at=(68, -46))
p.dimension(endrim[0], show_in="top", reference=True, id="end_dia_top", at=(42, 50))
p.dimension("arm/run1/along/v7", show_in="front", id="r10", at=(20, 10))
p.dimension(end30, datum(x0), reference=True, show_in="top", id="half_top")
p.dimension(end30, datum(x0), show_in="right", id="half_right")
p.dimension(tube_cyl, datum(x0), far=True, show_in="top", id="tube_r", at=(-63, -10))
p.dimension("slit_cut/side/e12", datum(x0), show_in="top", id="slit_half")
p.on_sheet("hex_cut", show_in="section_c", at=(30, 20))
p.dimension("arm/run2/side/e0", "arm/run2/side/e2", show_in="section_b", id="b16a")
p.dimension("arm/run2/side/e6", "arm/run2/side/e4", show_in="section_b", id="b16b")
p.dimension(endrim[0], "ring_ext/start", centres=True, along=axis("Z"), show_in="front", id="boss_height", at=(62, -12))
p.dimension(endrim[0], "ring_ext/side/e7#2", centres=True, along=axis("Y"), show_in="top", id="boss_offset", at=(0, 45))

# The sheet. A-A across the run to the ring, B-B across the leg, C-C along the boss (as the front view's cutting lines stand).
p.op("Section", id="across_run", along="Plane", plane=axis("Y"), offset=-26.0)
p.op("Section", id="across_leg", along="Plane", plane=axis("Z"), offset=80.0)
p.op("Section", id="along_boss", along="Plane", plane=axis("Y"), offset=28.0)
p.drawing(views=[view("front", "Front", (100, 97), style="HiddenDashed"),
                 projected("top", "front", "Above", 115, style="HiddenDashed"),
                 projected("right", "front", "Right", 100, style="HiddenDashed"),
                 section_view("section_a", "across_run", (235, 232), of="front", cut_only=True),
                 section_view("section_b", "across_leg", (305, 232), of="front", letter="B", cut_only=True, turn=90),
                 section_view("section_c", "along_boss", (345, 122), of="front", letter="C"),
                 view("iso", "Isometric", (335, 248), scale=(1, 3)), view("iso2", "IsometricBackLeft", (385, 248), scale=(1, 3))], scale=(2, 3),
                notes=[note(None, (0, 0), "THRU", dimension="tube_hole"), note(None, (0, 0), "THRU", dimension="hole_dia_front"),
                       note(None, (0, 0), "C'BORE", dimension="cbore_dia"), note(None, (0, 0), "C'BORE", dimension="cbore_depth"),
                       note(None, (0, 0), "HEX", dimension="hex_af"), note(None, (0, 0), "HEX", dimension="hex_cut")])
rep = p.build(out, render=True); rep.show()
print("mass g", rep.volume * DENS)
print(rep.drawings[0]["problems"])
