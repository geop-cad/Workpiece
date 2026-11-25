import math, os, sys
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from shapecraft import Part, axis, topo, material, datum, view, projected, note
here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, "6.shapecraft")
material(here, "1060 ALUM ALLOY", 2700)
p = Part(unit="Millimetre", material="1060 ALUM ALLOY")

# base 120 x 140 x 20, plan view
s = p.sketch("XY", show_in="top")
r = s.rect("r", -60, -70, 120, 140)
dg = s.line(r.points[0], r.points[2], construction=True)
s.midpoint(s.origin, dg)
s.distance_u(r.points[0], r.points[1], 120)
s.distance_v(r.points[1], r.points[2], 140)
p.extrude(s, 20, combine="NewBody", id="base")

rep = p.build(out)
side = rep.faces(where=lambda f: abs(f["normal"][2]) < 1e-6)
bottom = rep.faces(where=lambda f: f["flat"] and f["normal"][2] < -0.99)
p.op("Draft", id="draft", faces=[topo(f) for f in side], neutral=topo(bottom[0]), angle=10.0, flip=True)
rep = p.build(out)
corners = rep.edges(where=lambda e: e["straight"] and abs(e["from"][2]-e["to"][2]) > 10)
p.fillet(corners, 20, id="corners")
p.on_sheet("draft", on_drawing=False)
p.on_sheet("corners", show_in="top", at=(78, 36))
rep = p.build(out)

# ramp ribs, side view (y, z): the 10 degree ends continue the draft, 30 degree ramp up to the lug
g = p.sketch("YZ", show_in="side")
P = {n: g.point(*c, name=n) for n, c in dict(a=(-70,0), b=(70,0), c=(66.5,20), d=(0,58), f=(-66.5,20)).items()}
L = lambda x, y: g.line(P[x], P[y])
lab, lbc, lcd, ldf, lfa = L("a","b"), L("b","c"), L("c","d"), L("d","f"), L("f","a")
fc = g.line(P["c"], P["f"], construction=True)
g.horizontal(lab); g.midpoint(g.origin, lab)
g.symmetric(P["c"], P["f"], "v")
g.point_on(P["d"], "v")
g.distance_u(P["a"], P["b"], 140, on_drawing=False)
g.distance_v(P["b"], P["c"], 20)
q = g.point(66.5, 0)
ax = g.line(q, P["c"], construction=True); g.point_on(q, lab); g.vertical(ax)
g.angle(ax, lbc, 10, show_in="side", at=(-66, -3))
g.angle(lcd, fc, 30, show_in="side", at=(-30, -4))
p.extrude(g, 20, combine="Join", symmetric=True, id="rib")

# lug, front view (u = z, v = x): sides at 10 degrees continuing the drafted base, round head with the bore
l = p.sketch("ZX", show_in="front")
p0 = l.point(0,-60); p1 = l.point(0,60); t1 = l.point(140,40); t2 = l.point(140,-40); c = l.point(100,0)
l1 = l.line(p0,p1); l2 = l.line(p1,t1); l3 = l.line(t2,p0)
arc = l.arc(c, t1, t2, cw=True)
l.point_on(p0, "v")
l.symmetric(p0, p1, "u"); l.symmetric(t1, t2, "u")
l.distance_v(p0, p1, 120, on_drawing=False)
l.tangent(l2, arc)
q = l.point(0, 40)
cl = l.line(q, t1, construction=True); l.point_on(q, l1); l.horizontal(cl)
l.angle(l2, cl, 10, show_in="front", at=(-47, 34))
l.distance_u(l.origin, c, 100)
hole = l.circle(c, 25); l.diameter(hole, 50, at=(38, 28))
p.extrude(l, 20, combine="Join", symmetric=True, id="lug")
p.on_sheet("lug", show_in="top")
p.on_sheet("rib", show_in="top")
rep = p.build(out)
flat0 = lambda e: e["straight"] and e["from"][2] == 0 and e["to"][2] == 0
along_y = rep.edges(where=lambda e: flat0(e) and abs(abs(e["from"][1]) - 70) < 1e-6 and abs(e["to"][1] - e["from"][1]) < 1e-6)
along_x = rep.edges(where=lambda e: flat0(e) and abs(abs(e["from"][0]) - 60) < 1e-6 and abs(e["to"][0] - e["from"][0]) < 1e-6)
p.dimension(*along_y, show_in="side", reference=True)
p.dimension(*along_x, show_in="front", reference=True)
lugface = rep.faces(where=lambda f: f["flat"] and f["normal"][1] > 0.99 and abs(f["middle"][1] - 10) < 1e-6 and f["middle"][2] > 20)
ribface = rep.faces(where=lambda f: f["flat"] and f["normal"][0] > 0.99 and abs(f["middle"][0] - 10) < 1e-6 and f["middle"][2] > 20)
print(len(lugface), len(ribface))
p.dimension(lugface[0], datum("ZX"), show_in="top", reference=True)
p.dimension(ribface[0], datum("YZ"), show_in="top", reference=True)

p.mass_properties()
colinear = [e for b in rep.bodies for e in b["edges"] if (lambda e: e["straight"] and abs(e["from"][1] + 66.5) < 0.1 and abs(e["to"][1] + 66.5) < 0.1 and abs(e["from"][2] - 20) < 1e-6 and e["from"][0] > 0)(e)][0]
t10 = math.tan(math.radians(10))
oneface = [f for b in rep.bodies for f in b["faces"] if f["flat"] and f["normal"][0] > 0.9 and f["middle"][0] > 40][0]
on = lambda y, z: (60 - z * t10, y, z)
corner = [e for b in rep.bodies for e in b["edges"] if not e["straight"] and abs(e["from"][2] - e["to"][2]) < 1 and e["middle"][2] < 1 and e["middle"][0] > 40 and e["middle"][1] < -40][0]
p.drawing(scale=(1, 2), centre_lines=["all", {"of": "ZX", "view": "top", "label": "CL SYM", "label_at": "start"}, {"of": "YZ", "view": "front", "label": "CL SYM", "label_at": "end"}, {"of": "ZX", "view": "side", "label": "CL SYM"}], views=[view("front","Right",(100,105)), projected("top","front","Above",95), projected("side","front","Right",125, style="HiddenDashed"), view("iso","IsometricFrontLeft",(330,228)), view("iso2","Dimetric",(340,108))],
  notes=[note(None, (0, 0), "(NON-CONICAL) (4) PLACES", dimension="corners"),
         note("iso", (-40, -64), "TRUE R20 (NON-CONICAL) (4) PLACES", to=corner),
         note("iso", (-60, -50), "COLINEAR EDGE", to=colinear), note("iso", (52, -36), "ONE FACE", to=[on(0, 90), on(-30, 8), on(20, 8)]),
         note(None, (0,0), "THRU", dimension="sketch3/d28")])
rep = p.build(out, render=True); rep.show()
print("mass g", rep.volume*2.7e-3)
