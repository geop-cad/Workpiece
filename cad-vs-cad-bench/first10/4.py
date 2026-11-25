import os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import Part, material, view, projected, detail_view, topo, axis, note
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "4.workpiece")
R10, R8, OFF = 29, 53, 25
material(here, "ABS 1020", 1020)
p = Part(material="ABS 1020")

# Front profile on ZX (u = z, v = x): base slice + tapered plate with its window; extruded 26 along +y.
# On ZX a line along x is "vertical" (constant u) and one along z is "horizontal"; arcs run the other way round.
s = p.sketch("ZX", id="plate_sk")
def pt(n, x, z): return s.point(z, x, n)
pt("L",-57.5,25); pt("R",57.5,25); pt("m",0,25)
pt("T1",27.2,146.8); pt("T2",-27.2,146.8); pt("C",0,140)
pt("Bbr",23.7,37); pt("Cbr",23.7,45); pt("Sr",31.5,46.9); pt("Tr",21.35,87.7); pt("Cw",0,82.4)
pt("Tl",-21.35,87.7); pt("Sl",-31.5,46.9); pt("Bbl",-23.7,37); pt("Cbl",-23.7,45)
lbot = s.line("L","R"); lr = s.line("R","T1"); ll = s.line("T2","L")
atop = s.arc("C","T1","T2",cw=True)
wb = s.line("Bbl","Bbr"); wr = s.line("Sr","Tr"); wl = s.line("Tl","Sl")
abr = s.arc("Cbr","Bbr","Sr",cw=True); atw = s.arc("Cw","Tr","Tl",cw=True); abl = s.arc("Cbl","Sl","Bbl",cw=True)
s.vertical(lbot); s.vertical(wb)
s.tangent(lr, atop); s.tangent(ll, atop)
s.tangent(wr, abr); s.tangent(wb, abr); s.tangent(wl, abl); s.tangent(wb, abl)
s.tangent(wr, atw); s.tangent(wl, atw)
s.parallel(wr, lr); s.parallel(wl, ll)
s.point_on("C", "u"); s.point_on("Cw", "u"); s.point_on("m", "u"); s.midpoint("m", lbot)
s.distance_u(s.origin, "L", 25)
s.distance_v("L", "R", 115, on_drawing=False)
s.distance_u(s.origin, "C", 140, at=(-53, -9))
s.radius(atop, 28, show_in="detail", at=(-36, 42)); s.radius(atw, 22, at=(-38, 13)); s.radius(abl, 8, at=(40, -14)); s.equal(abr, abl)
s.distance_u("L", "Bbl", 12, at=(51, -39))
s.point_line("Tl", ll, 20, at=(24, -1))
p.extrude(s, 26, combine="NewBody", id="plate")
p.on_sheet("plate", show_in="left", at=(-31, 49))

# Base: 115 x 80 plan, R10 on the two front corners, 25 high.
b = p.sketch("XY", id="base_sk", show_in="top")
for n, x, y in [("p0",-57.5,0),("p1",57.5,0),("p2",57.5,70),("p3",47.5,80),("p4",-47.5,80),("p5",-57.5,70),
                ("c1",47.5,70),("c2",-47.5,70)]:
    b.point(x, y, n)
bl = [b.line("p0","p1"), b.line("p1","p2"), b.line("p3","p4"), b.line("p5","p0")]
a1 = b.arc("c1","p2","p3"); a2 = b.arc("c2","p4","p5")
b.horizontal(bl[0]); b.horizontal(bl[2]); b.vertical(bl[1]); b.vertical(bl[3])
b.tangent(bl[1], a1); b.tangent(bl[2], a1); b.tangent(bl[2], a2); b.tangent(bl[3], a2)
b.radius(a1, 10, at=(42, -36)); b.equal(a1, a2)
b.midpoint(b.origin, bl[0]); b.distance_u("p0", "p1", 115, at=(-27, 0)); b.distance_v("p0", "p4", 80)
p.extrude(b, 25, id="base")
p.on_sheet("base", on_drawing=False)

# Boss: R24 cylinder (R28 less the 4 offset) 50 long from the plate's back face; its centre dot drills the Ø26 hole.
c = p.sketch("ZX", id="boss_sk", on_drawing=False)
c.point(140, 0, "c")
c.point_on("c", "u"); c.distance_u(c.origin, "c", 140)
c.project("r28", "plate/side/e29")
bc = c.circle("c", 24); c.distance(bc, "r28", 4, on_drawing=False)
d = c.dot(140, 0); c.coincident(d, "c")
p.extrude(c, 50, id="boss")
p.hole(c, 26, id="bore")
p.on_sheet("bore/callout", show_in="detail", at=(34, 40))
p.on_sheet("boss", show_in="left", at=(-7, 59))


# Ø35 vertical hole, centre 25 from the front edge.
h = p.sketch("XY", id="hole_sk", on_drawing=False)
h.point(0, 80, "e")
h.point_on("e", "v"); h.distance_v(h.origin, "e", 80)
hd = h.dot(0, 55); h.point_on(hd, "v"); h.distance_v(hd, "e", 25, show_in="top", at=(32, 34))
p.hole(h, 35, next=True, id="vert")
p.on_sheet("vert/callout", show_in="top", at=(56, 23))  # stops where it first comes out: the base only, not the boss above
r = p.build(out)
p.dimension("plate/edge[boss/side/e21]", "plate/end/e29", id="offset", show_in="detail", at=(40, 20))
p.dimension("base/side/e16", "base/side/e18", reference=True, show_in="front", at=(1, -64))

# Two V grooves under the base, 60 degree flanks, 10 deep, cut through along x.
g = p.sketch("YZ", id="groove_sk", show_in="left")
g.point(12, 0, "a"); g.point(23.5, 0, "b"); g.point(17.8, 10, "m1")
g.point(52.5, 0, "c"); g.point(64, 0, "d"); g.point(58.2, 10, "m2"); g.point(80, 0, "e")
ab = g.line("a","b"); am = g.line("a","m1"); bm = g.line("m1","b")
cd = g.line("c","d"); cm = g.line("c","m2"); dm = g.line("m2","d")
g.horizontal(ab); g.horizontal(cd)
g.point_on("a", "u"); g.point_on("e", "u"); g.point_on("c", "u")
g.distance_u(g.origin, "a", 12, at=(-30, -59)); g.angle(ab, am, 60, at=(20, -18)); g.angle(bm, ab, 60, at=(-42, -24)); g.distance_v("a", "m1", 10, at=(-37, -50))
g.distance_u(g.origin, "e", 80, on_drawing=False); g.distance_u("d", "e", 16, at=(31, -59))
g.angle(cd, cm, 60, at=(40, -24)); g.angle(dm, cd, 60, at=(-22, -18)); g.distance_v("c", "m2", 10, at=(38, -50))
p.extrude(g, 10, combine="Cut", through=True, symmetric=True, id="grooves")
p.drawing(scale=(1, 2), views=[view("front", "Right", (201, 142), style="HiddenDashed"),
    projected("left", "front", "Left", 124, style="HiddenDashed"), projected("top", "left", "Above", 96),
    detail_view("detail", "front", (0, 56), 32, (201, 232), scale=(1, 1)),
    view("iso", "Isometric", (345, 218)), view("iso2", "IsometricBelowBackRight", (345, 112))],
    notes=[note(None, (0, 0), "(2) PLACES", dimension="base_sk/d%d" % R10), note(None, (0, 0), "(2) PLACES", dimension="plate_sk/d%d" % R8),
           note(None, (0, 0), "HOLE", dimension="bore/callout"), note(None, (0, 0), "HOLE", dimension="vert/callout"),
           note(None, (0, 0), "OFFSET", dimension="offset")])
r = p.build(out, render=True); r.show(); print(r.volume, r.volume*0.00102)
