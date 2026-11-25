# Rocking lid (25-WC-37). Modelled as the drawing builds it: a solid puck (O.D. 8.5, 1.5 high) with a
# U-shaped lug at each rib (front view: R.5 about the sphere centre, 1.125 up, to the rim; plan: its
# outer face .3 OFFSET from the O.D., concentric), R.5 round on the bottom, then shelled .125 open at
# the top -- which gives the (R.375) inside round and the lug's .125 walls, open to the lid's inside.
# Sleeve from its top (2.2) down to the lid's inside ("up to next": its foot stays inside the R.5 round,
# as the front view's outline shows) and the hole; two R.25 domes on the lug faces (9.5 sphere to sphere); a .125 rib (45 deg, top at
# 1.125, 7 across the top corners) running into the lug, patterned to the other side.
import sys, math, os, json
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from workpiece import *
here = os.path.dirname(os.path.abspath(__file__))
material(here, "ABS 1020", 1020)   # the drawing's density
RHO = 1020 * 3.6127e-5  # 1020 kg/m^3 in lb/in^3
p = Part(unit="Inch", material="ABS 1020")

import math
def camera_view(eye, up=(0, 0, 1)):
    """A camera-view `view` argument: parallel projection looking from direction `eye` (model axes), `up` up."""
    n = lambda v: [c / math.sqrt(sum(x * x for x in v)) for c in v]
    cross = lambda a, b: [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
    z = n(eye); x = n(cross(up, z)); y = cross(z, x)
    m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]]
    t = m[0][0] + m[1][1] + m[2][2]
    if t > 0:
        s = math.sqrt(t + 1) * 2; w = s / 4; qx = (m[2][1]-m[1][2])/s; qy = (m[0][2]-m[2][0])/s; qz = (m[1][0]-m[0][1])/s
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        s = math.sqrt(1 + m[0][0] - m[1][1] - m[2][2]) * 2; w = (m[2][1]-m[1][2])/s; qx = s/4; qy = (m[0][1]+m[1][0])/s; qz = (m[0][2]+m[2][0])/s
    elif m[1][1] > m[2][2]:
        s = math.sqrt(1 + m[1][1] - m[0][0] - m[2][2]) * 2; w = (m[0][2]-m[2][0])/s; qx = (m[0][1]+m[1][0])/s; qy = s/4; qz = (m[1][2]+m[2][1])/s
    else:
        s = math.sqrt(1 + m[2][2] - m[0][0] - m[1][1]) * 2; w = (m[1][0]-m[0][1])/s; qx = (m[0][2]+m[2][0])/s; qy = (m[1][2]+m[2][1])/s; qz = s/4
    return {"target": [0.0, 0.0, 0.0], "rotation": [qx, qy, qz, w], "distance": 300.0, "fov_y": 0.5, "orthographic": True}

s = p.sketch("YZ", id="lid_sk", show_in="front")
sh = s.polyline([(0, 0), (4.25, 0), (4.25, 1.5), (0, 1.5)], True, "l")
P, L = sh.points, sh.curves
s.coincident(P[0], s.origin)
s.horizontal(L[0]); s.vertical(L[1]); s.horizontal(L[2]); s.point_on(P[3], "v")
s.diameter_about(P[1], "v", 8.5, show_in="plan", decimals=1, at=(-50, -40))
s.distance_v(P[1], P[2], 1.5, show_in="side", decimals=1)
p.revolve(s, "v", combine="NewBody", id="lid")
u = p.sketch("YZ", id="lug_sk", show_in="front")
u.project("rim", "lid/start/e13")
c = u.point(0, 1.125); t1 = u.point(-.5, 1.5); t2 = u.point(.5, 1.5); b1 = u.point(-.5, 1.125); b2 = u.point(.5, 1.125)
top = u.line(t1, t2); rgt = u.line(t2, b2); lft = u.line(b1, t1); arc = u.arc(c, b1, b2)
u.point_on(c, "v"); u.distance_v(u.origin, c, 1.125, at=(74, -8))
u.radius(arc, .5, at=(-45, -22), decimals=1); u.tangent(rgt, arc); u.tangent(lft, arc); u.vertical(rgt); u.vertical(lft)
u.point_on(t1, "rim"); u.point_on(t2, "rim")
u.distance_u(u.origin, t2, .5, reference=True, show_in="plan", decimals=1, at=(19, 46), id="half_r")
u.distance_u(t1, u.origin, .5, reference=True, show_in="plan", decimals=1, at=(-19, 46), id="half_l")
# both lugs at once: the U through the puck both ways, then kept within the .3 offset circle
p.extrude(u, 1, through=True, symmetric=True, combine="Join", target=topo("lid"), id="lugs")
o = p.sketch("XY", id="lug_od_sk", show_in="plan")
o.project("od", "lid/ring/p8")
oc = o.circle(o.origin, 4.55)
o.distance("od", oc, .3, decimals=1, at=(48, -40), id="offset")
p.op("Extrude", "lug_od", sketch=profile(o), extent="ThroughBoth", combine="Intersect", target=topo("lid"))
p.fillet(["lid/ring/p8"], 0.5, id="round_outer")
p.shell(["lid/side/e13"], 0.125, id="wall")
p.plane("Z", 2.2, id="sleeve_top")
sl = p.sketch(datum("sleeve_top"), id="sleeve_sk", show_in="plan")
sc = sl.point(0, 3); sl.point_on(sc, "v"); sl.distance_v(sl.origin, sc, 3.0, decimals=0, at=(30, -68))
sc_o = sl.circle(sc, 1.25); sl.diameter(sc_o, 2.5, decimals=1, at=(72, 42), id="sleeve_dia")
hd = sl.dot(0, 3); sl.coincident(hd, sc)
p.op("Extrude", "sleeve", sketch=profile(sl), extent="ToNext", flip=True, combine="Join")
p.hole(sl, 2.25, id="bore")
p.plane("Z", 1.125, id="sphere_plane")  # the sphere centre and rib top (also the lug's R.5 centre)
p.on_sheet("sphere_plane", show_in="secv", at=(72, 6))
q = p.sketch(datum("sphere_plane"), id="dome_sk", show_in="side")
cR = q.point(4.5, 0); pR = q.point(4.75, 0); tR = q.point(4.5, .25)
cL = q.point(-4.5, 0); pL = q.point(-4.75, 0); tL = q.point(-4.5, .25)
aR = q.arc(cR, pR, tR); vR = q.line(tR, cR); q.line(cR, pR)
aL = q.arc(cL, tL, pL); q.line(pL, cL); vL = q.line(cL, tL)
for pt in (cR, pR, cL, pL): q.point_on(pt, "u")
q.vertical(vR); q.vertical(vL)
q.symmetric(pL, pR, "v")
q.radius(aR, .25, show_in="plan", decimals=2, at=(-40, -70), id="dome_r"); q.equal(aR, aL)
q.distance_u(pL, pR, 9.5, decimals=1, id="sphere_gap")
p.revolve(q, "u", id="domes")
g = p.sketch("ZX", id="rib_sk", show_in="secv")
g.project("seam", "domes/start/e18")
# two runs of one Rib, mirror images across the lid's axis plane: A to C is the right rib (45 degrees at its slope), the 7 across the two top corners
A = g.point(0.125, 2.5); B = g.point(1.125, 3.5); C = g.point(1.125, 4.5)
A2 = g.point(0.125, -2.5); B2 = g.point(1.125, -3.5); C2 = g.point(1.125, -4.5)
ab = g.line(A, B); bc = g.line(B, C)
bc2 = g.line(B2, C2); ab2 = g.line(A2, B2)
g.coincident(C, "seam_b")
g.vertical(bc); g.project("sl", "lid/edge[sleeve/side/e9#2]"); g.vertical(A, "sl_a")
g.symmetric(A, A2, "u"); g.symmetric(B, B2, "u"); g.symmetric(C, C2, "u")
g.distance_v(B2, B, 7, decimals=0, at=(0, -26))
g.angle(ab2, "u", 45, decimals=0, at=(44, 24))
p.op("Rib", "rib", sketch=profile(g), thickness=0.125)
p.on_sheet("round_outer", show_in="front", decimals=1, at=(-70, -30))
p.on_sheet("wall", show_in="front", at=(-64, 34))
p.on_sheet("rib", show_in="plan", at=(-34, -46))
p.on_sheet("sleeve_top", show_in="front", at=(58, 26), decimals=1)
p.on_sheet("bore", show_in="plan", decimals=2, at=(78, -24))
p.dimension("lid/side/e12", kind="Diameter", reference=True, show_in="secv", decimals=1, at=(0, -40), id="od_ref")
p.dimension("lid/side/e12", "thick/f1/far#2#2", reference=True, show_in="front", at=(-56, 22), decimals=3, id="wall_ref")
p.dimension("thick/f12/far#2#2", kind="Radius", reference=True, show_in="front", at=(-60, -45), decimals=3, id="r375")
p.op("Section", "secA", plane=axis("Y"))
p.op("CameraView", "under", looking="Current", view=camera_view((1, -1, -1)))
D = "HiddenDashed"
p.drawing(format="A3", scale=(1, 2), views=[view("plan", "Top", (110, 215), style=D), projected("front", "plan", "Below", 125, style=D),
          section_view("secv", "secA", (300, 205), of="front"), projected("side", "front", "Right", 125, style=D),
          view("iso", "Isometric", (340, 85), scale=(1, 5), style="Shaded"),
                 view("iso_under", "under", (290, 270), scale=(1, 5), style="Shaded"),
                 view("iso_cut", "Isometric", (360, 270), scale=(1, 5), style="Shaded", section="secA")],
          notes=[note(None, (0, 0), "SLEEVE", dimension="sleeve_sk/sleeve_dia"),
                 note(None, (0, 0), "HOLE", dimension="bore"),
                 note(None, (0, 0), "OFFSET", dimension="lug_od_sk/offset"),
                 note(None, (0, 0), "WALL TYP", dimension="wall"),
                 note(None, (0, 0), "RIB CENTERED", dimension="rib"),
                 note(None, (0, 0), "SPHERICAL CENTERED", dimension="dome_sk/dome_r"),
                 note(None, (0, 0), "SPHERE to SPHERE", dimension="dome_sk/sphere_gap"),
                 note("secv", (88, 17), "TO CENTER\nOF SPHERE\nAND TOP\nOF RIB"),
                 note(None, (0, 0), "O.D.", dimension="od_ref"),
                 note("plan", (62, -86), "SPHERES and RIBS\nCENTERED ON PART", to=(4.7, 0, 1.2)),
                 note("iso", (8, 30), "CONCENTRIC", to=[(4.7, 0, 1.2), (-4.7, 0, 1.2)])])
r = p.build("10.wpc", render=True); r.show(); print("mass lb", r.volume * RHO)
