import sys, math
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from shapecraft import *
from shapecraft import view, projected, section_view
p = Part(unit="Inch", material="6061 Alloy")
RHO = 0.097543  # 2700 kg/m^3 in lb/in^3

# --- body: a slot 4.0 between centres, 4.5 wide (R2.25 ends about the tower and the dome), 2.5 high
b = p.sketch("XY", id="body_sk", show_in="plan")
sl = b.slot("b", 0, 0, 4.0, 4.5)
b.coincident(sl.points[0], b.origin); b.horizontal(sl.points[0], sl.points[1])
p.extrude(b, 2.5, combine="NewBody", id="body")
p.on_sheet("body", show_in="side")

# --- tower: dia 1.750 up to 6.125, and the dome: dia 2.0 up to 4.000 at 4.0 along
t = p.sketch("XY", id="tower_sk", show_in="front")
tc = t.circle(t.origin, 0.875); t.diameter(tc, 1.75)
p.extrude(t, 6.125, id="tower")
p.on_sheet("tower", show_in="side")
d = p.sketch("XY", id="dome_sk", show_in="front")
dp = d.point(4, 0); d.point_on(dp, "u"); d.distance_u(d.origin, dp, 4.0, on_drawing=False)
dc = d.circle(dp, 1.0); d.diameter(dc, 2.0)
p.extrude(d, 4.0, id="dome")
p.on_sheet("dome", show_in="side")

# --- mounting plate on the tower: 3.25 x 5.0, R.75 corners, .375 thick, top at 5.750
top = p.plane("Z", 5.75, id="plate_top")
q = p.sketch(datum(top), id="plate_sk", show_in="plan")
c1 = q.dot(.875, 1.75); c2 = q.dot(-.875, 1.75); c3 = q.dot(-.875, -1.75); c4 = q.dot(.875, -1.75)
P = {k: q.point(*v) for k, v in dict(r1=(1.625, 1.75), r2=(.875, 2.5), l2=(-.875, 2.5), l3=(-1.625, 1.75),
     l4=(-1.625, -1.75), b3=(-.875, -2.5), b4=(.875, -2.5), r4=(1.625, -1.75)).items()}
a1 = q.arc(c1, P["r1"], P["r2"]); a2 = q.arc(c2, P["l2"], P["l3"]); a3 = q.arc(c3, P["l4"], P["b3"]); a4 = q.arc(c4, P["b4"], P["r4"])
tp = q.line(P["r2"], P["l2"]); lf = q.line(P["l3"], P["l4"]); bt = q.line(P["b3"], P["b4"]); rt = q.line(P["r4"], P["r1"])
q.horizontal(tp); q.horizontal(bt); q.vertical(lf); q.vertical(rt)
for l, a in [(tp, a1), (tp, a2), (lf, a2), (lf, a3), (bt, a3), (bt, a4), (rt, a4), (rt, a1)]: q.tangent(l, a)
q.radius(a1, 0.75); q.equal(a1, a2); q.equal(a1, a3); q.equal(a1, a4)
dg = q.line(c1, c3, construction=True); q.midpoint(q.origin, dg)
q.distance_u(P["l3"], P["r1"], 3.25); q.distance_v(P["b3"], P["r2"], 5.0, show_in="side")
p.extrude(q, 0.375, flip=True, id="plate")
p.on_sheet("plate", on_drawing=False)


# --- R.625 on the body's top edge and the dome's top edge, then R.25 at every concave junction
r = p.build("9-as-answered.shapecraft")
purple = r.edges(where=lambda e: e["name"].startswith("body/end/") or e["name"].startswith("dome/end/"))
p.fillet(purple, 0.625, id="fillet_big")
p.on_sheet("fillet_big", on_drawing=False)
r = p.build("9-as-answered.shapecraft")
base = r.edges(where=lambda e: e["name"] in ("body/edge[end+tower/side/e7]", "body/edge[dome/side/e8+end]"))
p.fillet(base, 0.25, id="fillet_small")
p.on_sheet("fillet_small", on_drawing=False)
r = p.build("9-as-answered.shapecraft")
# AS ANSWERED: the accepted model (Ricardo, video 6:52-6:57) also opened the dome's flat top in the shell.
openings = r.faces(where=lambda f: f["flat"] and any(abs(f["middle"][2] - z) < 1e-6 for z in (0, 4.0, 6.125)))
p.shell(openings, 0.375, id="shell")
p.on_sheet("shell", on_drawing=False)
r = p.build("9-as-answered.shapecraft")
# --- base flange: 5.5 wide, 10.0 long tangent to tangent, R2.0 tips, 65 degree sides, R.5 corners.
# It is added after the shell (a ring around the body's foot, whose inside is the body's outline
# projected), so the wall runs straight down to the bottom, sharp, as section A-A shows.
s = p.sketch("XY", id="flange_sk", show_in="plan")
D = s.point(4, 0)                       # the dome's centre (to dimension from)
CL = s.point(-1, 0); CR = s.point(5, 0)  # centres of the two R2.0 tips
EL = s.point(-3, 0); ER = s.point(7, 0)  # the tips' extreme points
AU = s.point(-2.813, 0.845); AL = s.point(-2.813, -0.845)   # left tip tangent points
BU = s.point(6.813, 0.845); BL = s.point(6.813, -0.845)
KUL = s.point(-1.605, 2.25); KLL = s.point(-1.605, -2.25); KUR = s.point(5.605, 2.25); KLR = s.point(5.605, -2.25)
UL1 = s.point(-2.058, 2.464); LL1 = s.point(-2.058, -2.464); UR1 = s.point(6.058, 2.464); LR1 = s.point(6.058, -2.464)
UL2 = s.point(-1.605, 2.75); LL2 = s.point(-1.605, -2.75); UR2 = s.point(5.605, 2.75); LR2 = s.point(5.605, -2.75)
tipL = s.arc(CL, AU, AL); tipR = s.arc(CR, BL, BU)
cLL = s.arc(KLL, LL1, LL2); cLR = s.arc(KLR, LR2, LR1); cUR = s.arc(KUR, UR1, UR2); cUL = s.arc(KUL, UL2, UL1)
sLL = s.line(AL, LL1); bot = s.line(LL2, LR2); sLR = s.line(LR1, BL); sUR = s.line(UR1, BU); top = s.line(UR2, UL2); sUL = s.line(AU, UL1)
s.horizontal(bot); s.horizontal(top)
for l, a in [(sLL, tipL), (sLL, cLL), (bot, cLL), (bot, cLR), (sLR, cLR), (sLR, tipR),
             (sUR, tipR), (sUR, cUR), (top, cUR), (top, cUL), (sUL, cUL), (sUL, tipL)]:
    s.tangent(l, a)
s.point_on(CL, "u"); s.point_on(CR, "u")
s.point_on(EL, tipL); s.point_on(EL, "u"); s.point_on(ER, tipR); s.point_on(ER, "u")
s.radius(tipL, 2.0); s.equal(tipL, tipR)
s.radius(cUL, 0.5); s.equal(cUL, cUR); s.equal(cUL, cLL); s.equal(cUL, cLR)
s.angle("u", sUL, 65); s.angle(sUR, "u", 65, on_drawing=False); s.angle(sLL, "u", 65, on_drawing=False); s.angle("u", sLR, 65, on_drawing=False)

wd = s.line(LL2, UL2, construction=True); M = s.point(-1.6, 0); s.midpoint(M, wd); s.point_on(M, "u")
s.distance_v(LL2, UL2, 5.5)
s.distance_u(D, ER, 3.0); s.distance_u(EL, ER, 10.0, show_in="front")
# The inside of the ring: the body's foot, projected (no number repeated).
for k, e in dict(pt="body/start/e13", pb="body/start/e14", pl="body/start/e15", pr="body/start/e16").items():
    s.project(k, e)
i1 = s.point(0, 2.25); i2 = s.point(4, 2.25); i3 = s.point(4, -2.25); i4 = s.point(0, -2.25)
s.line(i1, i2); s.line(i4, i3); s.arc(CL0 := s.point(0, 0), i1, i4); s.arc(D, i3, i2)
s.coincident(CL0, "pl_center"); s.coincident(D, "pr_center")
s.coincident(i1, "pt_a"); s.coincident(i2, "pt_b"); s.coincident(i4, "pb_a"); s.coincident(i3, "pb_b")
p.extrude(s, 0.375, id="flange")
p.on_sheet("flange", show_in="front")

r = p.build("9-as-answered.shapecraft")
foot = lambda e: e["name"].startswith("flange/end/") and abs(e["middle"][1]) < 2.25 + 1e-6 and -2.25 - 1e-6 < e["middle"][0] < 6.25 + 1e-6
# AS ANSWERED: no R.25 at the flange foot (absent on the accepted model, video 6:52).
rest = r.edges(where=lambda e: e["name"] in ("body/edge[plate/start+tower/side/e7]", "body/edge[plate/end+tower/side/e7#2]"))
assert len(rest) == 2, rest
p.fillet(rest, 0.25, id="fillet_outer")
p.on_sheet("fillet_outer", on_drawing=False)
p.hole(q, 0.5, depth=0.375, id="plate_holes")
p.on_sheet("plate_holes", on_drawing=False)
# Numbers the sketches do not carry.
midx = p.plane("X", 0, id="midx")
m = p.op("Dimension", "half_plate", of=[topo("plate/side/e20"), datum(midx)], kind="Distance")
p._dimension_views[m] = "plan"
p.dimension("body/side/e13", "body/side/e14", show_in="plan")
p.dimension("body/side/e16", kind="Radius", show_in="plan")
p.dimension("tower/side/e7", kind="Diameter", show_in="front")
for f, where in {"fillet_big/e1": (-85, -12), "fillet_big/e4": (45, 20), "fillet_outer/e0": (-70, 45),
                 "fillet_outer/e1": (-70, 30), "fillet_small/e0": (-75, 8),
                 "fillet_small/e1": (28, -14)}.items():
    p.dimension(f + "/side/e0", kind="Radius", show_in="front", at=where)
p.dimension("flange/start", "plate/start", show_in="side", id="plate_top_z")
p.dimension("flange/start", "plate/end", show_in="side", id="plate_bottom_z")
p.dimension("flange/start", "flange/end", show_in="side")
p.dimension("thick/f7/far", kind="Diameter", show_in="secv")
p.dimension("body/end", "thick/f5/far", reference=True, show_in="secv")
p.dimension("fillet_small/e1/side/e0", kind="Radius", show_in="secv")
p.dimension("fillet_big/e4/side/e0", kind="Radius", show_in="secv")
p.op("Section", "secA", plane=axis("Y"))
p.drawing(format="A2", scale=(1, 2), views=[view("front", "Left", (160, 100)), projected("plan", "front", "Above", 120),
          section_view("secv", "secA", (330, 230), of="plan"), projected("side", "front", "Right", 150)],
          notes=[note("plan", (-30, 105), "Ø.500 HOLE (4) PLACES"), note("secv", (0, 55), "THRU"), note("secv", (60, -45), "SHARP"),
                 note("secv", (60, -10), "WALL TYP"), note("front", (0, -35), "TANGENT TO TANGENT")])
r = p.build("9-as-answered.shapecraft", render=True); r.show(); print("mass lb", r.volume * RHO)
