"""Two parts and an assembly: an aluminium plate with two holes, a steel pin placed in each.

Each item of the parts list is a part of its own (plate.workpiece, pin.workpiece); the assembly
(pinned_plate.workpiece) places the plate once and the same pin file twice, and the report gives
what each placed part and all of it weighs. Run: python3 pinned_plate.py [folder]
"""
import math, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "python"))  # a checkout; `pip install workpiece` makes this line unnecessary
from workpiece import Part, anchor, placed, topo, datum

folder = sys.argv[1] if len(sys.argv) > 1 else "pinned_plate"
os.makedirs(folder, exist_ok=True)

# The plate: 60 x 40 x 10, two Ø10 holes 40 apart on its middle line.
plate = Part(material="Aluminium", file="plate.workpiece")
s = plate.sketch("XY", show_in="top")
r = s.rect("r", -30, -20, 60, 40)
s.symmetric(r.points[0], r.points[1], "v")
s.symmetric(r.points[1], r.points[2], "u")
s.distance_u(r.points[0], r.points[1], 60)
s.distance_v(r.points[1], r.points[2], 40)
plate.extrude(s, 10, combine="NewBody", id="plate")
h = plate.sketch("XY", show_in="top")
a, b = h.dot(-20, 0, "a"), h.dot(20, 0, "b")
h.horizontal(a, h.origin), h.horizontal(h.origin, b)
h.symmetric(a, b, "v")
h.distance(a, b, 40)
plate.hole(h, 10, id="holes")

# The pin: Ø10 x 30, standing on the origin.
pin = Part(material="Steel", file="pin.workpiece")
c = pin.sketch("XY", show_in="top")
c.diameter(c.circle(c.origin, 5), 10)
pin.extrude(c, 30, combine="NewBody", id="pin")

# The assembly: the plate fixed where it was modelled, each pin fixed through a hole,
# standing 5 out under the plate (its origin at the hole's centre, 5 below the plate).
asm = Part()
asm.add_part(plate, id="plate")
for k, x in enumerate((-20, 20)):
    asm.add_part(pin, id=f"pin{k + 1}", at=(x, 0, -5))
report = asm.build(os.path.join(folder, "pinned_plate.workpiece"))
report.show()
assert report.ok, report.errors

# A placed part's faces by name, where it is in the assembly: the top of the plate, the
# top end of the first pin — a reference into a placed part says which part it is of.
top = report.faces(placed="plate", where=lambda f: f["flat"] and f["normal"][2] > 0.99)
end = report.faces(placed="pin1", near=(-20, 0, 25))
print("plate top:", top, " pin1 top end:", end, " as a reference:", placed("pin1", topo(end[0])))

# A third pin held by a joint instead of a pose: its bottom face fastened onto the middle
# of the plate's top, facing it — it stands on the plate there.
bottom = report.faces(placed="pin1", near=(-20, 0, -5))
asm.add_part("pin.workpiece", id="pin3", constraint="Fastened", flip=True,
             onto=anchor(topo(top[0]), "FaceCentre", instance="plate"),
             with_=anchor(topo(bottom[0]), "FaceCentre"))
report = asm.build(os.path.join(folder, "pinned_plate.workpiece"))
assert report.ok, report.errors
pin3 = report.placed_part("pin3")["pose"]["at"]
print("pin3 stands at", [round(x, 6) for x in pin3])
assert math.dist(pin3, (0, 0, 10)) < 1e-6, pin3

# A fourth one fastened the same way, but 12 along x and 8 along y off the top's middle (in the
# top face's own frame: x and y in its plane, z along its normal) — no helper frame needed.
asm.add_part("pin.workpiece", id="pin4", constraint="Fastened", flip=True,
             onto=anchor(topo(top[0]), "FaceCentre", instance="plate"),
             with_=anchor(topo(bottom[0]), "FaceCentre"), offset=(12, 8, 0))
# And how tall it stands over the plate, measured between the two placed parts.
asm.dimension(placed("plate", topo(top[0])), placed("pin4", topo(end[0])), id="pin4_height")
report = asm.build(os.path.join(folder, "pinned_plate.workpiece"))
assert report.ok, report.errors
pin4 = report.placed_part("pin4")["pose"]["at"]
print("pin4 stands at", [round(x, 6) for x in pin4])
assert math.dist(pin4, (12, 8, 10)) < 1e-6, pin4
height = next(st["says"] for st in report.steps if st["id"] == "pin4_height")
print("pin4 stands", height, "over the plate")
assert height == "30", height

# What it weighs, checked by hand (mm³ x kg/m³ x 1e-9 = kg).
plate_volume = 60 * 40 * 10 - 2 * math.pi * 5 ** 2 * 10
pin_volume = math.pi * 5 ** 2 * 30
by_hand = {"plate": plate_volume * 2700e-9, "pin1": pin_volume * 7850e-9,
           "pin2": pin_volume * 7850e-9, "pin3": pin_volume * 7850e-9, "pin4": pin_volume * 7850e-9}
for m in report.masses:
    print(f"{m['name']:<6} {m['file']:<12} {m['material']:<10} {m['mass'] * 1000:9.3f} g"
          f"   by hand {by_hand[m['name']] * 1000:9.3f} g")
    assert abs(m["mass"] - by_hand[m["name"]]) < 1e-9, m
assert abs(report.mass_total - sum(by_hand.values())) < 1e-9
print(f"in all {report.mass_total * 1000:.3f} g (by hand {sum(by_hand.values()) * 1000:.3f} g)")
# Mass with a density the script gives, for a part whose material the library does not have.
assert abs(report.mass(7850e-9, placed="pin1") - by_hand["pin1"]) < 1e-12
