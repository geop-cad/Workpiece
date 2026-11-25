import sys, os
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from shapecraft import *
from shapecraft import view, projected
here = os.path.dirname(os.path.abspath(__file__))
material(here, "ABS 1020", 1020)   # the drawing's density
p = Part(unit="Millimetre", material="ABS 1020")

# C channel, 59 long along y: the profile on ZX (u = z, v = x), extruded symmetric about the origin plane.
def P(x, z): return s.point(z, x)
s = p.sketch("ZX", id="channel_profile", show_in="right")
A0 = P(-25, 0); A1 = P(-25, 8); A2 = P(-15, 18)
B2 = P(15, 18); B1 = P(25, 8); B0 = P(25, 0)
I0 = P(17, 0); I1 = P(17, 8); I2 = P(15, 10)
J2 = P(-15, 10); J1 = P(-17, 8); J0 = P(-17, 0)
Cl = P(-15, 8); Cr = P(15, 8)
oL = s.arc(Cl, A2, A1, cw=True); oR = s.arc(Cr, B1, B2, cw=True); iR = s.arc(Cr, I1, I2, cw=True); iL = s.arc(Cl, J2, J1, cw=True)
lA = s.line(A0, A1); lT = s.line(A2, B2); lB = s.line(B1, B0); lBR = s.line(B0, I0)
lI = s.line(I0, I1); lW = s.line(I2, J2); lJ = s.line(J1, J0); lBL = s.line(J0, A0)
for l in (lA, lB, lI, lJ): s.horizontal(l)
for l in (lT, lBR, lW, lBL): s.vertical(l)
s.tangent(lA, oL); s.tangent(lT, oL); s.tangent(lT, oR); s.tangent(lB, oR)
s.tangent(lI, iR); s.tangent(lW, iR); s.tangent(lW, iL); s.tangent(lJ, iL)
s.symmetric(Cl, Cr, "u"); s.point_on(A0, "v"); s.vertical(A0, B0)
s.radius(oR, 10, at=(-52, 40)); s.radius(iR, 2, at=(-52, 30))
s.distance_v(A0, B0, 50, show_in="top"); s.distance_u(A0, A2, 18, show_in="front")
p.extrude(s, 59, combine="NewBody", symmetric=True, id="channel")
p.on_sheet("channel", show_in="top")
r = p.build("7.shapecraft")

# The four pockets: one rectangle inside the walls (4 at the sides, 8 at the ends) cut by the cross of the two gaps (4 across, 8 along);
# only the four corner areas are cut, the cross stays as ribs.
q = p.sketch("XY", id="pocket_sketch", show_in="bottom")
q.project("ex", "channel/along/p17"); q.project("ey", "channel/end/e24")
o = q.rect("o", -21, -21.5, 42, 43)
q.symmetric(o.points[0], o.points[1], "v"); q.symmetric(o.points[1], o.points[2], "u")
q.point_line(o.points[2], "ey", 8, at=(-24, 36)); q.point_line(o.points[1], "ex", 4, at=(40, 18))
# gap 4 between the two long ribs' lines, gap 8 between the two cross lines
va = q.point(2, -21.5); vb = q.point(2, 21.5); wa = q.point(-2, -21.5); wb = q.point(-2, 21.5)
v1 = q.line(va, vb); v2 = q.line(wa, wb)
q.vertical(v1); q.vertical(v2); q.symmetric(va, wa, "v"); q.symmetric(vb, wb, "v")
q.point_on(va, o.curves[0]); q.point_on(vb, o.curves[2]);
q.distance_u(wa, va, 4, at=(-40, 0))
ha = q.point(-21, 4); hb = q.point(21, 4); ia = q.point(-21, -4); ib = q.point(21, -4)
h1 = q.line(ha, hb); h2 = q.line(ia, ib)
q.horizontal(h1); q.horizontal(h2); q.symmetric(ha, ia, "u"); q.symmetric(hb, ib, "u")
q.point_on(ha, o.curves[3]); q.point_on(hb, o.curves[1]);
q.distance_v(ia, ha, 8, at=(0, -36))
p.cut(q, 14, areas=[(12, 12), (-12, 12), (12, -12), (-12, -12)], id="pocket")
p.on_sheet("pocket", on_drawing=False)
r = p.build("7.shapecraft")
rim = r.edges(where=lambda e: e["straight"] and abs(e["middle"][2] - 14) < 1e-6 and abs(abs(e["middle"][0]) - 21) < 1e-6)
p.fillet(rim, 6, id="pocket_round")
p.on_sheet("pocket_round", show_in="right", at=(-52, 50))
# Boss and hub: one cylinder, 20 high from the boss top (z = 26) down.
top = p.plane("Z", 26, id="boss_top")
h = p.sketch(datum(top), id="hub_sketch")
c = h.circle(h.origin, 11); h.diameter(c, 22, show_in="right")
p.extrude(h, 20, flip=True, id="hub")
p.on_sheet("hub", show_in="right")
# Counterbored bore from the boss top.
b = p.sketch(datum(top), id="bore_sketch")
d = b.dot(0, 0); b.coincident(d, b.origin)
p.hole(b, 10, kind="Counterbore", head=16, head_depth=4, id="bore")
p.on_sheet("bore", show_in="top")
r = p.build("7.shapecraft")
# R6 on the four leg ends.
legs = r.edges(where=lambda e: e["straight"] and abs(e["middle"][2]) < 1e-6 and abs(abs(e["middle"][1]) - 29.5) < 1e-6 and abs(e["from"][0]-e["to"][0]) > 1)
print(legs)
p.fillet(legs, 6, id="leg_round")
p.on_sheet("leg_round", show_in="front", at=(-55, -33))
# Numbers the sketches do not carry, between the faces they measure.
wall_pocket = r.faces(near=(21, 12.8, 4), where=lambda f: f["flat"] and f["name"].startswith("pocket/side"))[0]
leg_outer = r.faces(near=(25, 0, 4), where=lambda f: f["flat"] and f["name"].startswith("channel/side"))[0]
leg_inner = r.faces(near=(17, 0, 4), where=lambda f: f["flat"] and f["name"].startswith("channel/side"))[0]
p.dimension("hub/side/e7#2", kind="Diameter", reference=True, show_in="top", at=(-50, 14), id="hub_ref")
p.on_sheet("bore/callout", show_in="top", at=(56, -30))
p.dimension("hub/side/e7#2", kind="Diameter", show_in="front", id="hub_dia")
p.dimension("bore/side/e1", kind="Diameter", reference=True, show_in="right", at=(52, 42), id="cbore_ref")
p.dimension("hub/start", "channel/side/e22", reference=True, show_in="right", id="boss_wall")
p.dimension(wall_pocket, leg_outer, show_in="right", id="wall4")
p.dimension(leg_outer, leg_inner, show_in="right", id="wall8")
wall_pocket_end = r.faces(near=(-13.9, -21.5, 8), where=lambda f: f["flat"] and f["name"].startswith("pocket/side"))[0]
p.dimension(wall_pocket_end, "channel/start", reference=True, show_in="bottom", id="end_ref", at=(40, 43))
leg_outer_l = r.faces(near=(-25, 0, 4), where=lambda f: f["flat"] and f["name"].startswith("channel/side"))[0]
wall_pocket_l = r.faces(near=(-21, 12.8, 4), where=lambda f: f["flat"] and f["name"].startswith("pocket/side"))[0]
p.dimension(wall_pocket_l, leg_outer_l, reference=True, show_in="bottom", id="side_ref", at=(-32, -34))
leg_inner_l = r.faces(near=(-17, 0, 4), where=lambda f: f["flat"] and f["name"].startswith("channel/side"))[0]
p.dimension(leg_outer_l, leg_inner_l, reference=True, show_in="bottom", id="leg_ref", at=(40, -30))
p.drawing(thru=True, views=[view("front", "Front", (90, 140), style="HiddenDashed"), projected("top", "front", "Above", 80, style="HiddenDashed"),
                 projected("bottom", "front", "Below", 80, style="HiddenDashed"),
                 projected("right", "front", "Right", 90, style="HiddenDashed"), view("iso", "Isometric", (280, 252)), view("iso_under", "IsometricBelowFrontRight", (362, 188)),
                 view("iso_under2", "IsometricBelowBackLeft", (300, 108))],
          notes=[note("iso_under", (-30, 42), "COPLANAR", to=[(0, 12, 0), (0, -12, 0), (12, 0, 0)]), note(None, (0, 0), "(4) PLACES", dimension="leg_round"), note(None, (0, 0), "C'BORE", dimension="cbore_ref"),
                 note(None, (0, 0), "WALL", dimension="wall4"), note(None, (0, 0), "WALL", dimension="wall8")])
r = p.build("7.shapecraft", render=True); r.show(); print(r.mass(1.02e-3))
