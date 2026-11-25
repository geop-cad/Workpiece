import os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from shapecraft import Part, axis, topo, material, view, projected, detail_view, note, datum
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "8.shapecraft")
material(here, "PLAIN CARBON STEEL", 7800)
p = Part(unit="Millimetre", material="PLAIN CARBON STEEL")

w = p.sketch("YZ", show_in="front")
c0 = w.circle(w.point(0, 9, "k0"), 3); w.diameter(c0, 6, on_drawing=False); w.point_on("k0", "v")
w.distance("u", c0, 12, far=True, show_in="side")
c1 = w.circle(w.point(25, 9, "k1"), 3); w.equal(c1, c0); w.horizontal("k0", "k1"); w.distance_u("k0", "k1", 25, on_drawing=False)
g2 = w.point(50, 9, "k2"); w.horizontal("k1", "k2"); w.distance_u("k1", "k2", 25, on_drawing=False)
c3 = w.circle(w.point(75, 9, "k3"), 3); w.equal(c3, c0); w.horizontal("k2", "k3"); w.distance_u("k2", "k3", 25, on_drawing=False)
n1 = w.circle(w.point(-25, 9, "m1"), 3); w.equal(n1, c0); w.symmetric("k1", "m1", "v")
n3 = w.circle(w.point(-75, 9, "m3"), 3); w.equal(n3, c0); w.symmetric("k3", "m3", "v")
p.extrude(w, 252, combine="NewBody", symmetric=True, id="wires")
p.on_sheet("wires", on_drawing=False)
rep = p.build(out)
cap = "wires/start"
rim = rep.edges(where=lambda e: abs(e["middle"][0] + 126) < 1e-3 and abs(e["middle"][1] - 22) < 1e-3)[0]

# Bent wire, one of the two, 50 off the middle (25 beyond the wire at 25): the path in a plane there.
pl25 = p.plane("Y", 25, id="pl25"); p.on_sheet("pl25", on_drawing=False)
pl50 = p.plane(datum("pl25"), 25, id="pl50"); p.on_sheet("pl50", on_drawing=False)
q = p.sketch(datum(pl50), id="path", show_in="side")
def vee(c):
    return [(z, x) for x, z in [(c - 26.44, 0), (c - 26.44, 9), (c - 20.08, 6.36), (c - 6.364, -7.364), (c, -1),
                                (c + 6.364, -7.364), (c + 20.08, 6.36), (c + 26.44, 0), (c + 26.44, 9)]]
names = ["o1", "a1s", "a1e", "a2s", "o2", "a2e", "a3s", "o3", "a3e"]
for tag, c in (("v", -50), ("w", 50)):
    for n, uv in zip(names, vee(c)):
        q.point(*uv, name=tag + n)
pe0 = q.point(9, -126, "p0"); pe1 = q.point(9, 126, "p1")
def vlines(tag):
    t = lambda n: tag + n
    L = {}
    L["a1"] = q.arc(t("o1"), t("a1s"), t("a1e"))
    L["d1"] = q.line(t("a1e"), t("a2s"))
    L["a2"] = q.arc(t("o2"), t("a2s"), t("a2e"), cw=True)
    L["d2"] = q.line(t("a2e"), t("a3s"))
    L["a3"] = q.arc(t("o3"), t("a3s"), t("a3e"))
    return L
V = vlines("v"); W = vlines("w")
l0 = q.line(pe0, "va1s"); f = q.line("va3e", "wa1s"); l1 = q.line("wa3e", pe1)
chain = [l0, V["a1"], V["d1"], V["a2"], V["d2"], V["a3"], f, W["a1"], W["d1"], W["a2"], W["d2"], W["a3"], l1]
for a, b in zip(chain, chain[1:]):
    q.tangent(a, b)
q.vertical(l0); q.collinear(l0, f); q.collinear(f, l1)
top = q.line(q.point(12, 0, "t0"), q.point(12, 40, "t1"), construction=True); q.vertical(top); q.point_on("t0", "u"); q.horizontal("t1", "vo2")
q.distance_u(q.origin, "t0", 12, on_drawing=False); q.point_line(pe0, top, 3, on_drawing=False)
q.distance_v(pe0, pe1, 252, on_drawing=False); q.symmetric(pe0, pe1, "u")
outer = q.circle("vo2", 12, construction=True); q.radius(outer, 12, on_drawing=False)
q.distance(outer, V["a2"], 3, on_drawing=False)
for a in [V["a2"], V["a3"], W["a1"], W["a2"], W["a3"]]:
    q.equal(V["a1"], a)
cl2 = q.line("wa1s", "va3e", construction=True)
q.angle(W["d1"], cl2, 135, show_in="bend", at=(-30, 10)); q.angle(cl2, W["d2"], 135, show_in="bend", at=(30, 10))
q.parallel(V["d1"], W["d1"]); q.parallel(V["d2"], W["d2"])
q.distance(top, outer, 25, far=True, show_in="side", at=(88, 0))
q.equal(V["d1"], W["d1"])
q.distance_v("vo2", "wo2", 100, on_drawing=False)
vw = q.line("vo2", "wo2", construction=True); mm = q.point(-1, 0, "mm"); q.point_on(mm, "u"); q.midpoint(mm, vw)

# Its section: a circle on the end of the wire at 25, the next wire's width (25) beyond it.
t = p.sketch(topo(cap), id="section", on_drawing=False)
t.project("rimc", rim)
tc = t.circle(t.point(-50, 9, "tp"), 3); t.equal(tc, "rimc")
t.horizontal("rimc_center", "tp"); t.distance_u("rimc_center", "tp", 25)
p.sweep(t, q, combine="NewBody", id="bent")
p.op("Transform", "bent2", motion="Mirror", body=topo("bent"), plane=datum("ZX"), copy=True)
rep = p.build(out); rep.show()

# Wires cut back to the ring (240 inside diameter), seen from the top.
k = p.sketch("XY", on_drawing=False)
inner = k.circle(k.origin, 120); k.diameter(inner, 240)
p.extrude(k, 50, combine="Intersect", through=True, symmetric=True, id="trim")
# The ring wire (added after the trim, which would cut it): a 6 mm circle inside the 252 outer diameter, revolved about z.
s = p.sketch("YZ", show_in="top")
ring = s.circle(s.point(123, 3, "rc"), 3, "ringwire"); s.diameter(ring, 6, reference=True)
od = s.circle(s.origin, 126, "od", construction=True); s.diameter(od, 252, at=(-66, 66))
s.tangent(ring, od, True); s.tangent(ring, "u")
s.distance_v(s.origin, "rc", 3, on_drawing=False)
p.revolve(s, about="SketchV", combine="NewBody", id="ring")

p.on_sheet("pl25", on_drawing=False); p.on_sheet("pl50", on_drawing=False); p.on_sheet("trim", on_drawing=False); p.on_sheet("bent2", on_drawing=False)
p.dimension("wires/side/e13", "wires/side/e14", same_side=True, show_in="top", id="pitch", at=(9, -84))
p.dimension("wires/side/e14", "bent/run6/side/e1", same_side=True, show_in="top", reference=True, id="pitch2", at=(44, 8))
p.dimension("wires/side/e13", kind="Radius", plain=True, reference=True, show_in="top", id="r3", at=(-14, -70))
p.dimension("wires/side/e13", kind="Diameter", show_in="top", id="dc", at=(-18, -77))
p.dimension("wires/side/e14", kind="Diameter", show_in="top", reference=True, id="dw", at=(66, -40))
p.dimension("ring/side/e9", kind="Diameter", bend="Inside", reference=True, show_in="top", id="ringin", decimals=0, at=(-56, 45))
p.dimension("bent/run9/side/e2", "wires/side/e14", far=True, along=axis("Z"), reference=True, plain=True, show_in="front", id="h25f")
AT = {(11, "o"): (-18, 35), (11, "i"): (-30, 25), (7, "o"): (18, 35), (7, "i"): (30, 25), (9, "o"): (26, -28), (9, "i"): (-26, -28)}
for k in (7, 9, 11):
    for side_, bend_ in (("o", "Outside"), ("i", "Inside")):
        p.dimension(f"bent/run{k}/side/e0", kind="Radius", bend=bend_, show_in="bend", id=f"r{k}{side_}", at=AT[(k, side_)])
p.dimension("bent/run3/side/e0", "bent/run9/side/e0", centres=True, along=axis("X"), show_in="side", id="v100", at=(0, -22))
rep = p.build(out)
to_wire = [e for b_ in rep.bodies for e in b_["edges"] if e["name"] == "wires/edge[side/e16+trim/side/e7#3]"][0]
p.drawing(scale=(1, 2), views=[view("top", "Top", (100, 205)), projected("front", "top", "Below", 125),
          projected("side", "front", "Right", 160), detail_view("bend", "side", (-50, 0), 38, (222, 138), (1, 1), letter_at=(46, 30), style="HiddenDashed"),
          view("iso", "Isometric", (338, 214)), view("edge", direction=(0.4, 0.9, 0.14), at=(338, 270))],
          notes=[note("top", (45, -92), "25 SPACING BETWEEN WIRES TYP"), note("front", (-35, -18), "ALL WIRES ARE \u00d86", to=(0, -75, 9)),
                 note("top", (115, 61), "ALL WIRES CUT TO PERFECTLY MATCH", to=(89, 89)), note(None, (215, 261), "\u00d8252 O.D. RING (FROM TOP VIEW)"),
                 note("side", (-80, -40), "ALL BENDS ARE TANGENT", to=(50, 50, -13)),
                 note("iso", (-26, -40), "HINT:", to=(0, 126, 3)), note("iso", (-26, -45), "THERE ARE (7) CROSS WIRES"), note("iso", (-26, -50), "AND"), note("iso", (-26, -55), "(1) \u00d8252 O.D. RING WIRE"),
                 note("iso", (40, -48), "ALL WIRES ARE \u00d86", to=(0, 25, 9))])
rep = p.build(out, render=True); rep.show()
print("mass g", rep.volume*7.8e-3)
