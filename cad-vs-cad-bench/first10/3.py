# T handle 2 (26-WF-20), MMGS, ABS 1020 kg/m^3. Read as:
# - grip: Ø40 x 115 with R20 spherical ends, axis 188 above the base; bored Ø28 from both ends to a 6 web
#   with a Ø12 hole through it (one revolve and one revolve cut, about the grip axis on the plane z = 188);
# - frame: the front outline (shaft Ø40 up 50, R75 tangent to R105, R105 tangent to the VERTICAL side under
#   the grip end, up to the grip axis) extruded 40 and fully rounded R20 all along its sides;
# - the inside, 8 in from the outline (R97/R83 concentric with R105/R75), closed by R12.5 at the crotch; at the
#   top the 8-in line leaves R97 tangentially on an arc centred on the centre line that ends on the grip axis
#   at the spherical end's centre (the front view's curve from the side up to the grip end, fitted to 0.5 mm;
#   it is also what makes the side view's outline narrow smoothly from the grip's 40 to the frame's 32);
# - split by the D's lower arc (tangent to R97, bottom 93 up): below it pocketed from both sides to a 6 web
#   (the top view's hidden web lines end at |y| = 40, where the D's arc meets R97); above it the D is cut
#   through right up to the grip, so the grip's underside is the D's top (section A-A);
# - Ø30 socket 40 deep, flat bottomed.
# - R12 (drawing: section A-A corner and iso) is a 3D round of the edge where the D's arc wall meets each
#   spherical grip end (one contestant, Ace, filleted those edges R12 too). Each edge ends where the sphere,
#   the grip and the frame's full round are tangent: the round is trimmed by the full round's surface there
#   and fades out to nothing at that point.
import sys, math
sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parents[2] / "python"))
from shapecraft import *
import os
here = os.path.dirname(os.path.abspath(__file__))
material(here, "ABS 1020", 1020)
p = Part(material="ABS 1020")
RHO = 1020e-9  # kg/mm^3 (the drawing's ABS; the library ABS is 1050)
# grip: revolve about the grip axis (y) in the plane z = 188
p.plane("Z", 188, id="grip_plane")
g = p.sketch(datum("grip_plane"), id="grip_sk", show_in="front")
a0 = g.point(0, -37.5); a1 = g.point(20, -37.5); a2 = g.point(20, 37.5); a3 = g.point(0, 37.5)
e0 = g.point(0, -57.5); e3 = g.point(0, 57.5)
side = g.line(a1, a2); g.line(a3, e3); 
arcR = g.arc(a3, a2, e3)  # about a3 from (20,37.5) to (0,57.5)
axl = g.line(e3, e0)
arcL = g.arc(a0, e0, a1)
for q in (a0, a3, e0, e3): g.point_on(q, "v")
g.vertical(side)
g.tangent(side, arcR); g.tangent(side, arcL)
g.symmetric(e0, e3, "u")
g.diameter_about(a1, "v", 40, show_in="secv", at=(-40, 60))
g.distance_v(e0, e3, 115)
p.revolve(g, "SketchV", combine="NewBody", id="grip")
f = p.sketch("YZ", id="frame_sk", show_in="secv")
f.project("endL", "grip/start/e16"); f.project("endR", "grip/start/e17")
B0 = f.point(20, 0); B1 = f.point(-20, 0)
S1 = f.point(20, 50); S2 = f.point(-20, 50)
T1 = f.point(35.6, 95.8); T2 = f.point(-35.6, 95.8)
V1 = f.point(57.5, 160); V2 = f.point(-57.5, 160)
W1 = f.point(57.5, 188); W2 = f.point(-57.5, 188)
C75R = f.point(95, 50); C75L = f.point(-95, 50); C105R = f.point(-47.5, 160); C105L = f.point(47.5, 160)
bot = f.line(B1, B0); sR = f.line(B0, S1); sL = f.line(S2, B1)
r75R = f.arc(C75R, T1, S1); r105R = f.arc(C105R, T1, V1); vR = f.line(V1, W1); top = f.line(W1, W2)
vL = f.line(W2, V2); r105L = f.arc(C105L, V2, T2); r75L = f.arc(C75L, S2, T2)
f.horizontal(bot); f.vertical(sR); f.vertical(sL); f.vertical(vR); f.vertical(vL)
f.coincident(W1, "endR_b"); f.coincident(W2, "endL_a")
for a_, b_ in ((sR, r75R), (r75R, r105R), (r105R, vR), (sL, r75L), (r75L, r105L), (r105L, vL)): f.tangent(a_, b_)
f.symmetric(B0, B1, "v"); f.symmetric(S1, S2, "v")
f.point_on(B0, "u")
f.diameter_about(B0, "v", 40, show_in="front", at=(0, -60))
f.distance_v(B0, S1, 50, at=(-42, -42))
f.radius(r75L, 75, at=(-30, -21)); f.radius(r105L, 105, at=(-40, 12)); f.equal(r75R, r75L); f.equal(r105R, r105L)
p.extrude(f, 40, symmetric=True, combine="NewBody", id="frame")
p.plane("X", 3, id="web_plane")
k = p.sketch(datum("web_plane"), id="inner_sk", show_in="front")
for nm, ed in (("ptop", "frame/end/e31"), ("p105R", "frame/end/e34"), ("p105L", "frame/end/e35"),
               ("p75R", "frame/end/e33"), ("p75L", "frame/end/e36"), ("gR", "grip/start/e17"), ("gL", "grip/start/e16")):
    k.project(nm, ed)
P = {}
def pt(n, u, v): P[n] = k.point(u, v, name=n); return n
pt("ER", 37.5, 188); pt("EL", -37.5, 188); pt("cP", 0, 156)
pt("TP", 49.15, 151.8); pt("TPL", -49.15, 151.8)
pt("TD", 40.1, 118.3); pt("TDL", -40.1, 118.3); pt("T97", 29.3, 100.7); pt("T97L", -29.3, 100.7)
pt("T83", 12.43, 58.48); pt("T83L", -12.43, 58.48)
pt("c105R", -47.5, 160); pt("c105L", 47.5, 160); pt("c75R", 95, 50); pt("c75L", -95, 50)
pt("cD", 0, 137.4); pt("c12", 0, 59.76)
# the pocket's outline: 8 in from the frame's (R97 / R83, closed by R12.5 at the crotch), and at the top an arc
# about the centre line from R97 (tangent) up to the grip axis at the spherical end's centre
top = k.line("EL", "ER", name="ktop")
uR = k.arc("c105R", "TD", "TP", name="kuR"); lR = k.arc("c105R", "T97", "TD", name="klR"); aR = k.arc("c75R", "T97", "T83", name="kaR")
uL = k.arc("c105L", "TPL", "TDL", name="kuL"); lL = k.arc("c105L", "TDL", "T97L", name="klL"); aL = k.arc("c75L", "T83L", "T97L", name="kaL")
pR1 = k.arc("cP", "TP", "ER", name="kpR")
pL1 = k.arc("cP", "EL", "TPL", name="kpL")
bt = k.arc("c12", "T83L", "T83", name="kbt"); dA = k.arc("cD", "TDL", "TD", name="kD")
for a_, b_ in (("c105R", "p105R_center"), ("c105L", "p105L_center"), ("c75R", "p75R_center"), ("c75L", "p75L_center"),
               ("ER", "gR_a"), ("EL", "gL_b")):
    k.coincident(a_, b_)
k.point_on("cP", "v")
k.tangent(uR, pR1, internal=True); k.tangent(uL, pL1, internal=True)
k.tangent(lR, aR); k.tangent(lL, aL); k.tangent(aR, bt); k.tangent(aL, bt); k.tangent(dA, uR); k.tangent(dA, uL)
k.distance(uR, "p105R", 8, show_in="front", at=(48, -2))
k.radius(bt, 12.5, at=(24, -38))
k.distance(k.origin, dA, 93, at=(-44, -24))
# full round: the outline edges on both faces (not the bottom and not the top along the grip axis)
E = [f"frame/{s}/e{n}" for s in ("start", "end") for n in (28, 33, 34, 30, 32, 35, 36, 29)]
p.fillet(E, 20, id="full_round")
p.op("Extrude", "opening", sketch=profile(k), extent="ThroughBoth", combine="Cut", target=topo("frame"),
     areas=[(0, 140)])
p.op("Extrude", "pocket", sketch=profile(k), extent="ThroughAll", combine="Cut", target=topo("frame"),
     areas=[(0, 80)])
p.feature_pattern(["pocket/start"], 2, about=axis("Z"), id="pocket_pair")
b = p.sketch(datum("grip_plane"), id="bore_sk", show_in="secv")
b.project("tipL", "grip/start/e16"); b.project("tipR", "grip/start/e17")
B = b.polyline([(0, -57.5), (14, -57.5), (14, -3), (6, -3), (6, 3), (14, 3), (14, 57.5), (0, 57.5)], True, "b")
bp, bl = B.points, B.curves
for i in (0, 2, 4, 6): b.horizontal(bl[i])
for i in (1, 3, 5): b.vertical(bl[i])
b.point_on(bp[0], "v"); b.point_on(bp[7], "v")
b.coincident(bp[0], "tipL_start"); b.coincident(bp[7], "tipR_end")
b.diameter_about(bp[1], "v", 28, at=(-64, 61)); b.diameter_about(bp[3], "v", 12, show_in="side", at=(24, 34))
b.distance_v(bp[3], bp[4], 6, at=(16, 44))
b.symmetric(bp[2], bp[5], "u")
p.op("CombineBodies", "join_grip", target=topo("frame"), tool=topo("grip"))
p.fillet(["frame/edge[grip/side/e16+opening/side/e56]", "frame/edge[grip/side/e17+opening/side/e57]"], 12, id="corner_round")
p.revolve(b, "SketchV", combine="Cut", id="bores")
h = p.sketch("XY", id="socket_sk", show_in="secv")
hd = h.dot(0, 0); h.coincident(hd, h.origin)
p.hole(h, 30, depth=40, point=180, id="socket")
p.on_sheet("full_round", show_in="top", at=(28, -22))
p.on_sheet("socket", on_drawing=False)
p.dimension("socket/side/e1", id="socket_d", kind="Diameter", show_in="secv", at=(20, -61))
p.dimension("frame/side/e27", "socket/side/e2", show_in="secv", at=(26, -44))
p.on_sheet("corner_round", show_in="secv", at=(14, 14))
p.on_sheet("web_plane", on_drawing=False)
p.on_sheet("grip_plane", show_in="front", at=(-60, 0))
p.dimension("pocket/start", "pocket_pair/1/pocket/start", show_in="side", at=(21, -12))
p.dimension(datum("grip_plane"), datum("XY"), reference=True, show_in="side", at=(30, 0), id="h188")
p.dimension("grip/side/e14", kind="Diameter", reference=True, show_in="side", at=(26, 20), id="gripd")
p.op("Section", "A", plane=axis("X"))
p.drawing(format="A3", scale=(1, 2), comma=False, views=[view("front", "Front", (88, 140), style="HiddenDashed"),
          projected("top", "front", "Above", 105, style="HiddenDashed"),
          section_view("secv", "A", (188, 140), of="top"), projected("side", "front", "Right", 194, style="HiddenDashed"),
          view("iso", "Isometric", (322, 226), scale=(1, 2)), view("iso2", "IsometricFrontLeft", (374, 226), scale=(1, 2)),
          view("iso3", "IsometricBelowFrontRight", (357, 122), scale=(1, 2))],
          notes=[note("front", (-38, 8), "TANGENT", to=[(-40.1, 24.3), (40.1, 24.3)]),
                 note("front", (40, -30), "TANGENT", to=[(12.4, -35.5), (-12.4, -35.5)]),
                 note("front", (-40, -18), "TANGENT", to=[(-29.3, 6.7), (29.3, 6.7)]),
                 note("secv", (-54, 22), "VERTICAL", to=(0, -57.5, 174)), note("secv", (52, 28), "STRAIT SECTION", to=(0, 57.5, 174)),
                 note("secv", (-52, -6), "TANGENT", to=(0, -35.6, 95.8)), note("secv", (50, 2), "TANGENT", to=(0, 57.5, 160)),
                 note("secv", (44, -22), "TANGENT", to=(0, 20, 50)),
                 note("top", (50, 22), "R20 SPHERICAL", to=(-14.1, 51.6, 188)),
                 note(None, (0, 0), "THRU", dimension="bore_sk/d43")])
r = p.build("3.shapecraft", render=True); r.show()
print("mass g at 1020 kg/m^3:", r.volume * RHO * 1000)
