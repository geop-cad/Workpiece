import sys, os
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import *
from workpiece import view, projected
here = os.path.dirname(os.path.abspath(__file__))
material(here, "Plain carbon steel", 7800)   # the drawing's density
p = Part(unit="Millimetre", material="Plain carbon steel")

# Base plate: the plan outline (R16 about four hole centres, joined by tangent lines), 15 thick,
# hanging down from the datum plane at its top face (z = 15).
top = p.plane("Z", 15, id="plate_top")
p.on_sheet("plate_top", on_drawing=False)
s = p.sketch(datum(top), id="plate_sk", show_in="plan")
H1 = s.dot(-40, 35); H2 = s.dot(40, 35); H3 = s.dot(-40, -35); H4 = s.dot(40, -65)
T1 = s.point(-40, 51); L1 = s.point(-56, 35); L3 = s.point(-56, -35)
B3 = s.point(-45.6, -50); B4 = s.point(34.4, -80); R4 = s.point(56, -65); R2 = s.point(56, 35); T2 = s.point(40, 51)
a1 = s.arc(H1, T1, L1); a3 = s.arc(H3, L3, B3); a4 = s.arc(H4, B4, R4); a2 = s.arc(H2, R2, T2)
left = s.line(L1, L3); slant = s.line(B3, B4); right = s.line(R4, R2); tp = s.line(T2, T1)

for l, a in [(left, a1), (left, a3), (slant, a3), (slant, a4), (right, a4), (right, a2), (tp, a2), (tp, a1)]:
    s.tangent(l, a)
for a in (a1, a2, a3, a4): s.radius(a, 16)
s.symmetric(H1, H2, "v"); s.vertical(H1, H3); s.vertical(H2, H4)
s.point_line(s.origin, tp, 51, show_in="left")
s.distance_u(H1, H2, 80); s.distance_v(H3, H1, 70); s.distance_v(H4, H2, 100, at=(68, 0))
p.extrude(s, 15, combine="NewBody", flip=True, id="plate")
p.on_sheet("plate", show_in="front")
p.hole(s, 12, kind="Counterbore", head=20, head_depth=8, id="mount_holes")

# Lug A (bore 25): a 28 wide stem into R23 round, on the plane 12 in from the plate's left end.
r = p.build("8.wpc")
end_a = r.faces(where=lambda f: f["flat"] and f["name"].startswith("plate/side") and f["normal"][0] < -0.99)[0]
end_b = r.faces(where=lambda f: f["flat"] and f["name"].startswith("plate/side") and f["normal"][0] > 0.99)[0]
xa = p.plane(topo(end_a), -12, id="lug_a_plane")
a = p.sketch(datum(xa), id="lug_a_sk", show_in="left")
c = a.point(0, 70)
bl = a.point(-14, 0); br = a.point(14, 0); jl = a.point(-14, 52); jr = a.point(14, 52)
arc = a.arc(c, jr, jl)
lb = a.line(bl, br); ll = a.line(jl, bl); lr = a.line(br, jr)
a.vertical(ll); a.vertical(lr); a.horizontal(lb)
a.radius(arc, 23, at=(30, 52)); a.distance_u(bl, a.origin, 14); a.distance_v(bl, c, 70); a.point_on(c, "v")
a.distance_u(a.origin, br, 14); a.point_on(bl, "u")
bore = a.circle(c, 12.5); a.diameter(bore, 25, at=(45, 5))
p.extrude(a, 15, flip=True, id="lug_a")
p.on_sheet("lug_a", show_in="front")

# Lug B (bore 30): full width R23 round with straight sides.
xb = p.plane(topo(end_b), -12, id="lug_b_plane")
b = p.sketch(datum(xb), id="lug_b_sk", show_in="right")
c = b.point(0, 70)
bl = b.point(-23, 0); br = b.point(23, 0); tl = b.point(-23, 70); tr = b.point(23, 70)
arc = b.arc(c, tr, tl)
lb = b.line(bl, br); ll = b.line(tl, bl); lr = b.line(br, tr)
b.vertical(ll); b.vertical(lr); b.horizontal(lb)
b.tangent(ll, arc); b.tangent(lr, arc)
b.radius(arc, 23, at=(-45, 50)); b.point_on(c, "v"); b.distance_v(bl, c, 70); b.point_on(bl, "u")
bore = b.circle(c, 15); b.diameter(bore, 30, at=(-50, 5))
p.extrude(b, 15, flip=True, id="lug_b")
p.on_sheet("lug_b", show_in="front")

# Rib between the lugs: 8 wide at its top (z = 42), 8 degrees of draft each side down to the plate.
r = p.build("8.wpc")
inner_a = r.faces(near=(-29, 0, 40), where=lambda f: f["flat"] and f["name"].startswith("lug_a"))[0]
inner_b = r.faces(near=(29, 0, 40), where=lambda f: f["flat"] and f["name"].startswith("lug_b"))[0]
rib_plane = p.plane(topo(inner_a), 0, id="rib_plane")
g = p.sketch(datum(rib_plane), id="rib_sk", show_in="right")
g.project("plate_edge", r.edges(where=lambda e: e["straight"] and abs(e["middle"][2] - 15) < 1e-6 and abs(e["middle"][0] + 56) < 1e-6)[0])
bl = g.point(-7.8, 15); br = g.point(7.8, 15); tr = g.point(4, 42); tl = g.point(-4, 42)
lbot = g.line(bl, br); lrt = g.line(tr, br); ltop = g.line(tr, tl); llt = g.line(tl, bl)
g.horizontal(lbot); g.horizontal(ltop)
g.point_on(bl, "plate_edge")
g.distance_u(tl, g.origin, 4, at=(-14, -22)); g.distance_u(g.origin, tr, 4, at=(14, -22)); g.distance_v(g.origin, tl, 42, show_in="front", at=(0, -8))
# a construction line down the middle of the rib: the 8 degrees are measured from it, below the apex as the drawing shows them
M = g.point(0, 42); N = g.point(0, 15)
g.midpoint(M, ltop); g.point_on(N, lbot)
cv = g.line(M, N, construction=True); g.vertical(cv)
g.angle(cv, lrt, 8, at=(28, -64)); g.angle(llt, cv, 8, at=(2, -64))
p.op("Extrude", "rib", id="rib", sketch=profile(g), extent="ToFace", up_to=topo(inner_b), combine="Join")
p.on_sheet("rib", on_drawing=False)
r = p.build("8.wpc")
rib_top = r.edges(where=lambda e: e["name"].startswith("rib/start") and e["straight"] and abs(e["to"][2] - e["from"][2]) < 1e-6)[0]
rib_foot = r.edges(where=lambda e: e["name"].startswith("rib/along") and abs(e["from"][2] - 15) < 1e-6)
p.dimension(rib_top, show_in="plan", at=(-12, -26), id="rib_top")
p.dimension(rib_foot[0], rib_foot[1], reference=True, show_in="plan", decimals=2, at=(14, -30), id="rib_bottom")
mid = p.plane("Y", 0, id="mid")
p.on_sheet("mid", on_drawing=False)
p.dimension("plate/side/e22", datum(mid), show_in="right", at=(30, -54), id="plate_half")
p.dimension("plate/start", "mount_holes/side/e2", reference=True, show_in="front", id="cbore_depth", at=(-63, -36))
p.on_sheet("mount_holes", show_in="plan", at=(-80, 64))
D = "HiddenDashed"
p.drawing(format="A2", scale=(1, 1), views=[view("front", "Left", (300, 125), style=D), projected("plan", "front", "Above", 150, style=D),
          projected("left", "front", "Left", 150, style=D), projected("right", "front", "Right", 150, style=D),
          view("iso", "Isometric", (60, 345), scale=(1, 2), style="Shaded"), view("iso2", "IsometricBackRight", (530, 345), scale=(1, 2), style="Shaded")],
          comma=False, notes=[note(None, (0, 0), "C'BORE", dimension="cbore_depth"),
                              note(None, (0, 0), "RIB AT TOP", dimension="rib_top"), note(None, (0, 0), "RIB AT BOTTOM", dimension="rib_bottom"),
                              note("iso2", (-40, -62), "C'BORES ARE CONCENTRIC TO FILLETS", to=(-40, 35, 15))])
r = p.build("8.wpc", render=True); r.show(); print(r.mass(7.8e-3))
