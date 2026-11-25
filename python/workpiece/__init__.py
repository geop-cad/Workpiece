"""Build workpiece parts from a script.

    from workpiece import Part
    p = Part()
    s = p.sketch("XY")
    r = s.rect("a", 0, 0, 60, 40)          # four lines, already horizontal / vertical
    s.coincident(r.points[0], s.origin)     # pin it
    s.distance_u(r.points[0], r.points[1], 60)
    s.distance_v(r.points[1], r.points[2], 40)
    p.extrude(s, 10)
    report = p.build("plate.wpc")         # solves, builds, reports
    report.show()

A script is the model: sketches say their constraints and driving dimensions (the solver
places the geometry; the coordinates you give are only a starting guess), features refer to
sketches and to named faces and edges, repeats are patterns. `build` runs the `workpiece` command
line and returns what came of it: failed steps, each sketch's degrees of freedom, volumes
after every step, faces and edges by name, lint, and optionally pictures.

See README.md beside this file for the conventions (planes, arcs, names).
"""

from __future__ import annotations

import json
import math
import os
import re
import subprocess
import tempfile
import sys
import time
from dataclasses import dataclass, field

from . import _check as workpiece_check  # what a script declares of the original; scorecard; lint
from . import _images as workpiece_images  # crops and tiles
from . import _bin
from ._bin import __version__, command_line as _binary  # noqa: F401
from ._check import original  # noqa: F401  (re-exported: `from workpiece import original`)
from ._images import crop, tile  # noqa: F401


# --- references ------------------------------------------------------------------------------

def profile(sketch) -> dict:
    """A sketch, as what a feature is made of."""
    return {"Profile": sketch.id if isinstance(sketch, Sketch) else sketch}


def _section(s) -> dict:
    """A section of a sweep or a loft: a sketch (by itself or by id), or a reference."""
    if isinstance(s, dict) and "of" in s:
        return s
    ref = s if isinstance(s, dict) else profile(s)
    return {"of": ref}


def _curve(c) -> dict:
    """A curve a sweep or a loft goes along: a sketch's curves, or a reference (a curve body)."""
    return c if isinstance(c, dict) else profile(c)


def _area(a):
    """An area of a sketch as a step takes it: a (u, v) point inside it becomes
    {"inside": [u, v]}; anchors (a list of {"entity", "left", "at"}) stay as they are."""
    if isinstance(a, (tuple, list)) and len(a) == 2 and all(isinstance(x, (int, float)) for x in a):
        return {"inside": [float(a[0]), float(a[1])]}
    return a


def topo(name: str) -> dict:
    """A face, edge, vertex or body by its name (see Report.faces / Report.edges)."""
    return {"Topology": name}


def datum(name: str) -> dict:
    """A datum (plane, axis, frame) a step made, by the step's id; "XY", "YZ", "ZX" are the
    origin's planes (square to Z, X, Y)."""
    square_to = {"XY": "Z", "YZ": "X", "ZX": "Y", "YX": "Z", "ZY": "X", "XZ": "Y"}
    if name.upper() in square_to:
        return axis(square_to[name.upper()])
    return {"Datum": name}


def axis(letter: str, frame: str = "origin") -> dict:
    """An axis of a frame (the origin, or one an AddFrame step made): "X", "Y" or "Z"
    (as a plane: the one square to it)."""
    return {"FrameAxis": {"frame": frame, "axis": letter.upper()}}


def placed(instance, ref) -> dict:
    """A reference into a part placed in this one (an assembly): `ref` (`topo(name)`,
    `datum(...)`, `axis(...)`) as it is in that part, where the step `instance` placed it — a
    list of step ids for a part placed in a placed part (`["frame", "hinge"]`). For a step
    that measures or refers across parts (`p.dimension(placed("pin1", topo(face)), ...)`).
    (`anchor(..., instance=...)` says the same for `add_part`'s `onto=`.)"""
    path = [instance] if isinstance(instance, str) else list(instance)
    for name in reversed(path):
        ref = {"Placed": {"instance": name, "source": ref}}
    return ref


def anchor(ref, snap: str = "Datum", instance=None) -> dict:
    """A reference a mate holds a placed part by, for `mate(onto, with_)`: a reference
    (`datum("origin")`, `axis("Z")`, `topo(face_or_edge)`) and how a frame sits on it — snap
    "Datum" (a datum's own frame), "FaceCentre" (a face's middle, z its normal), "FaceAxis"
    (the axis of a round face), "EdgeMiddle", "Vertex", "ArcCentre". `instance` names the
    placed part it is in, as a list from the assembly down (`["box"]` for a face of the part
    the step "box" placed); none for the assembly's own, or — as `with_` — the placed part's."""
    made = {"source": ref, "snap": snap}
    if instance:
        made["instance"] = [instance] if isinstance(instance, str) else list(instance)
    return made


def seat() -> dict:
    """The seat of a standard part, as the `with_` of its mate: the frame on its axis where it
    naturally sits, z into it — a screw's ring under its head, a nut's or a washer's face, a pin's
    end, a bearing's side face. `mate(onto=<a face, or a hole's rim on one>, with_=seat())` stands
    it on that face (concentric with the hole), as the app's Standard part does by one pick."""
    return {"source": {"Datum": "seat"}, "snap": "Datum"}


_CONSTRAINTS = ("Fastened", "Revolute", "Slider", "Cylindrical", "Planar", "Ball")


def mate(onto, with_, constraint: str = "Fastened", flip_onto: bool = False, flip_with: bool = False,
         offset=None, angle=None, distance=None) -> dict:
    """One mate of a placed part, for `add_part(mates=[...])` and `standard_part(mates=[...])`: a
    reference `onto` what is in the assembly already and one `with_` of the placed part (both
    `anchor(...)`; `anchor(topo(face), "FaceCentre", instance="plate")` for a face of a part placed
    before), and the constraint between them — "Fastened" (one frame), "Revolute" (concentric and
    coincident: it turns about the axis), "Slider", "Cylindrical" (concentric), "Planar", "Ball".
    flip_onto / flip_with turn that reference over (its z the other way): two faces put against
    each other need one of them flipped (flip_with=True). offset=(x, y, z) holds it that far off
    `onto`, in that reference's own frame (for a face: x and y in its plane, z along its normal),
    after the mate lines the two up. angle=(lo, hi) degrees and distance=(lo, hi) limit what the
    constraint lets go. Several mates hold what one leaves free."""
    if constraint not in _CONSTRAINTS:
        raise ValueError(f"constraint {constraint!r}: one of {', '.join(_CONSTRAINTS)}")
    made = {"onto": onto, "with": with_, "kind": constraint}
    if flip_onto:
        made["flip_onto"] = True
    if flip_with:
        made["flip_with"] = True
    if offset is not None:
        made["offset"] = [float(x) for x in offset]
    if angle is not None:
        made["angle"] = [float(x) for x in angle]
    if distance is not None:
        made["distance"] = [float(x) for x in distance]
    return made


def _hold(mates, fixed) -> dict:
    """How a placed part is held: fixed (where it is put) unless said, when it has no mates."""
    mates = list(mates or [])
    for m in mates:
        if not isinstance(m, dict) or "kind" not in m:
            raise ValueError("each of mates is a mate(onto, with_, ...)")
    hold = {"mates": mates}
    if fixed if fixed is not None else not mates:
        hold["fixed"] = True
    return hold


def material(folder: str, name: str, density: float, colour=(180, 180, 186)) -> None:
    """Adds the material `name` (density in kg/m³) to the materials of `folder` (its
    `materials.json`, which the app reads too; the built-in ones if it has none yet), so that
    `Part(material=name)` saved there weighs what it is made of."""
    done = subprocess.run([_binary(), "materials", "--in", folder], capture_output=True, text=True)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip() or done.stdout.strip())
    library = json.loads(done.stdout)
    library["materials"] = [m for m in library["materials"] if m["name"] != name]
    library["materials"].append({"name": name, "density": float(density), "colour": list(colour)})
    with open(os.path.join(folder, "materials.json"), "w") as f:
        json.dump(library, f, indent=2)


# --- drawing views ---------------------------------------------------------------------------

SIDES = ("Front", "Back", "Left", "Right", "Top", "Bottom", "Isometric", "IsometricFrontLeft",
         "IsometricBackRight", "IsometricBackLeft", "Dimetric", "IsometricBelowFrontRight",
         "IsometricBelowFrontLeft", "IsometricBelowBackRight", "IsometricBelowBackLeft")


def camera_view(eye, up=(0, 0, 1)) -> dict:
    """A CameraView step's `view` argument: a parallel projection looking from the direction
    `eye` (model axes, towards who looks) with `up` up on the sheet."""
    n = lambda v: [c / math.sqrt(sum(x * x for x in v)) for c in v]
    cross = lambda a, b: [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
    z = n(eye); x = n(cross(up, z)); y = cross(z, x)
    m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]]
    t = m[0][0] + m[1][1] + m[2][2]
    if t > 0:
        q = math.sqrt(t + 1) * 2
        w, qx, qy, qz = q / 4, (m[2][1]-m[1][2])/q, (m[0][2]-m[2][0])/q, (m[1][0]-m[0][1])/q
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        q = math.sqrt(1 + m[0][0] - m[1][1] - m[2][2]) * 2
        w, qx, qy, qz = (m[2][1]-m[1][2])/q, q/4, (m[0][1]+m[1][0])/q, (m[0][2]+m[2][0])/q
    elif m[1][1] > m[2][2]:
        q = math.sqrt(1 + m[1][1] - m[0][0] - m[2][2]) * 2
        w, qx, qy, qz = (m[0][2]-m[2][0])/q, (m[0][1]+m[1][0])/q, q/4, (m[1][2]+m[2][1])/q
    else:
        q = math.sqrt(1 + m[2][2] - m[0][0] - m[1][1]) * 2
        w, qx, qy, qz = (m[1][0]-m[0][1])/q, (m[0][2]+m[2][0])/q, (m[1][2]+m[2][1])/q, q/4
    return {"target": [0.0, 0.0, 0.0], "rotation": [qx, qy, qz, w], "distance": 300.0, "fov_y": 0.5,
            "orthographic": True}


def view(id: str, side: str | None = None, at=None, scale=None, style: str = "HiddenRemoved", bodies=None,
         section: str | None = None, direction=None, up=(0, 0, 1), caption_at=None) -> dict:
    """A view of the model from a side ("Front", "Back", "Left", "Right", "Top", "Bottom"), or
    pictorial: "Isometric" (from the front, right and above), "IsometricFrontLeft",
    "IsometricBackRight", "IsometricBackLeft", "Dimetric" (ISO 5456-3), from below
    "IsometricBelowFrontRight", "IsometricBelowFrontLeft", "IsometricBelowBackRight",
    "IsometricBelowBackLeft" — or any other way of
    looking, by the id of a camera view step (`p.op("CameraView", "trimetric", looking=...)`),
    with its middle `at` (x, y) millimetres from the sheet's lower left corner.
    style: "HiddenRemoved", "HiddenDashed", "Wireframe", "Shaded". bodies: the names of the
    bodies it shows (all of them, if not said) — one body of several per view; of an assembly,
    placed parts by their id ("base"), or bodies of one as "base/plate"; a name that is none of
    these is a problem of the drawing. section: the id of a Section step that cuts the model open
    in this view — a pictorial section ("Isometric") with the cut hatched. Instead of a side,
    `direction=(x, y, z)` (towards who looks, model axes) with `up=(0, 0, 1)` looks from any way —
    a camera view the drawing makes itself: `view("edge", direction=(1, 1, 0.2), at=(330, 80))`.
    caption_at=(x, y): where its caption (a scale of its own, "(1:2)") stands — its middle, mm on
    the sheet from the view's middle (it moves with the view); else by itself, under the view."""
    kind = {"Base": {"side": side}} if side in SIDES else {"Named": {"view": side}}
    assert side is not None or direction is not None, "a view needs a side or a direction"
    if direction is not None:
        # (A camera view of its own, made by the drawing: looking from `direction`.)
        kind = {"Named": {"view": f"{id}_camera"}}
    made = {"id": id, "kind": kind, "at": list(at), "style": style,
            "bodies": list(bodies or [])}
    if direction is not None:
        made["_camera"] = camera_view(direction, up)
    if section:
        made["section"] = section
    if scale:
        made["scale"] = list(scale)
    _caption(made, caption_at)
    return made


def _caption(made: dict, caption_at) -> None:
    # (Where a view's caption stands, if it is said: mm from the view's middle.)
    if caption_at is not None:
        made["caption_at"] = [float(caption_at[0]), float(caption_at[1])]


def projected(id: str, of: str, beside: str, distance: float = 90.0, style: str = "HiddenRemoved",
              bodies=None, caption_at=None) -> dict:
    """A view projected from the view `of`, standing "Left", "Right", "Above" or "Below" of it
    and in line with it, its middle `distance` millimetres on the sheet from that view's middle
    (what it shows follows the sheet's projection). caption_at=(x, y): as `view`'s."""
    made = {"id": id, "kind": {"Projected": {"of": of, "beside": beside}}, "at": None,
            "distance": distance, "style": style, "bodies": list(bodies or [])}
    _caption(made, caption_at)
    return made


def detail_view(id: str, of: str, centre, radius: float, at, scale=(2, 1), letter: str = "B",
                style: str = "HiddenRemoved", letter_at=None, caption_at=None) -> dict:
    """A detail: what of the view `of` lies within `radius` (the model's lengths) of `centre`
    (x, y in the model's lengths from the middle of what that view shows: across and up as
    that view shows them), drawn at `scale` with its middle `at` (mm from the sheet's lower
    left corner). Of an assembly's view it shows the placed parts as that view does.
    Its letter stands by the circle in `of` by itself (outside it, clear of lines and words,
    on a short leader if it must stand off); `letter_at` (x, y) puts it there: mm on the sheet
    from the circle's middle. Its caption "B (5:1)" stands over it; caption_at=(x, y) puts its
    middle there instead: mm on the sheet from the detail's middle (it moves with the view)."""
    detail = {"of": of, "centre": list(centre), "radius": float(radius), "letter": letter}
    if letter_at is not None:
        detail["letter_at"] = [float(letter_at[0]), float(letter_at[1])]
    made = {"id": id, "kind": {"Detail": detail}, "at": list(at), "scale": list(scale), "style": style}
    _caption(made, caption_at)
    return made


def balloon(part: str, view: str, at, to=None) -> dict:
    """A balloon (ISO 6433) put by hand, for `p.drawing(..., balloons=[...])`: of the item the part
    placed as `part` is, in the view `view`, its circle's middle `at` (x, y) mm from the view's
    middle; its leader ends on the part where the view shows it, or at `to` (x, y) mm from the
    view's middle."""
    made = {"part": part, "view": view, "at": [float(at[0]), float(at[1])]}
    if to is not None:
        made["to"] = [float(to[0]), float(to[1])]
    return made


def note(view: str | None, at, text: str, height: float = 3.5, dimension: str | None = None,
         to=None) -> dict:
    """Words on the sheet — "(2) PLACES", "THRU" — with their middle `at` (x, y) millimetres
    from the middle of the view `view` (they move with it), or with view=None from the
    sheet's lower left corner. Pass a list of them as `p.drawing(..., notes=[...])`.
    With `dimension` (the id of the step or the sketch dimension that says it: "corners"
    for a fillet's R, "sketch1/d4"), the words belong to that dimension: they stand under its
    text, turned as it is, wherever it goes (`R32` over `(4) PLACES`); `at` then moves them
    off that place (usually (0, 0)), and `view` is not needed.
    `to`: a leader from the words to what they say something of (ISO 128-22) — a point of the
    model (x, y, z), an edge or a face as the build report has it (`rep.edges(...)[0]`: its
    middle), or a point of the view (x, y) in the model's lengths from the middle of what the
    view shows. It ends in an arrowhead where that is on an outline of the view, else in a dot
    (within a face). Needs `view`. A list of such places, `to=[a, b]`, gives the words a leader
    to each ("NOT TANGENT" with two arrows)."""
    made = {"view": view, "at": list(at), "text": text, "height": float(height)}
    if dimension is not None:
        made["dimension"] = dimension
    if to is not None:
        point = lambda t: [float(x) for x in (t["middle"] if isinstance(t, dict) else t)]
        many = isinstance(to, list) and len(to) > 0 and isinstance(to[0], (list, tuple, dict))
        made["to"] = [point(t) for t in to] if many else point(to)
    return made


def section_view(id: str, section: str, at, of: str | None = None, letter: str = "A",
                 cut_only: bool = False, bodies=None, turn: float = 0.0, scale=None,
                 style: str = "HiddenRemoved", half: str | None = None, half_only: bool = False,
                 caption_at=None) -> dict:
    """The model cut as the Section step `section` cuts it. With `of`, its cutting line with its
    arrows and letters shows in that view; put about level with that view (within 15 mm of its
    line) the section is drawn in line with it, anywhere else it stands where `at` says. Make the Section step with
    `p.op("Section", along="Plane", plane=axis("X"), offset=0.0)`. cut_only=True draws only what
    the cut meets — the cut faces, hatched, with their outline — and nothing behind it.
    turn: degrees it is turned on the sheet, counter-clockwise (90: what runs across stands
    upright) — a turned section stands where `at` says, in line with nothing.
    scale=(1, 3): a scale of its own (written beside its name: "A–A (1:3)"); else that of the
    view `of` (or the sheet's). The parts or bodies it cuts are hatched each their own way.
    half="Left" | "Right" | "Above" | "Below": a half section (ISO 128-3) — only that half of the
    view (from its middle line, the middle of the model) is cut, the other half is drawn as the
    model is seen uncut, the two divided by a centre line; with half_only=True the other half is
    not drawn at all (a symmetric part's section from its axis outward).
    caption_at=(x, y): where its caption "A–A" stands — its middle, mm on the sheet from the
    section view's middle (it moves with the view); else over it."""
    made = {"id": id, "kind": {"Section": {"of": of, "section": section, "letter": letter, "turn": float(turn)}},
            "at": list(at), "style": style, "cut_only": cut_only, "bodies": list(bodies or [])}
    if scale:
        made["scale"] = list(scale)
    if half:
        made["half"] = {"cut": half, "only": bool(half_only)}
    _caption(made, caption_at)
    return made


# --- sketches --------------------------------------------------------------------------------

@dataclass
class Shape:
    """What a helper drew: the names of its points and curves, in order."""
    points: list
    curves: list


class Sketch:
    """A sketch: named points, lines, circles and arcs, with constraints and dimensions.

    Coordinates are (u, v) in the sketch's plane and are a starting guess; what holds is what
    the constraints and dimensions say. `origin` is the fixed point (0, 0); "u" and "v" are the
    plane's axes (construction lines through the origin) to constrain against.
    """

    origin = "origin"
    u_axis = "u"
    v_axis = "v"

    def __init__(self, id: str, plane, on_drawing: bool = True, show_in: str | None = None):
        self.id = id
        self.plane = plane
        self.on_drawing = on_drawing
        self.show_in = show_in
        self._points: dict = {}
        self._lines: dict = {}
        self._circles: dict = {}
        self._arcs: dict = {}
        self._construction: list = []
        self._rotation_axes: dict = {}
        self._fixed: list = []
        self._dots: list = []
        self._constraints: list = []
        self._dimension_options: dict = {}
        self._projected: dict = {}
        self._n = 0

    def _name(self, given, prefix):
        if given:
            return given
        self._n += 1
        return f"{prefix}{self._n}"

    # geometry
    def point(self, u: float, v: float, name: str | None = None) -> str:
        name = self._name(name, "p")
        self._points[name] = [float(u), float(v)]
        return name

    def project(self, name: str, what) -> str:
        """Projects something of the model (as the steps before this sketch leave it) onto the
        sketch plane as fixed construction geometry to constrain against — so a line can end on
        a body's edge or be dimensioned from its corner with no number repeated. `what` is an
        edge's or a vertex's name from a report (or a reference such as `axis("Z")`, or
        `datum(plane_id)` for where that datum plane crosses the sketch: a line). A vertex
        gives the point `name`; an edge gives the curve `name` with its points `name_a`,
        `name_b` (a line's ends) or `name_center`, `name_start`, `name_end` (an arc's)."""
        self._projected[name] = topo(what) if isinstance(what, str) else what
        return name

    def dot(self, u: float, v: float, name: str | None = None) -> str:
        """A point that stands for itself: where a hole goes (`Part.hole` drills at the dots
        of its sketch). Place it with dimensions like any point."""
        name = self.point(u, v, name)
        self._dots.append(name)
        return name

    def line(self, a: str, b: str, name: str | None = None, construction: bool = False) -> str:
        name = self._name(name, "l")
        self._lines[name] = [a, b]
        if construction:
            self._construction.append(name)
        return name

    def rotation_axis(self, point: str, towards, name: str | None = None) -> str:
        """The axis the sketch is turned about: a centre line without end through the point
        `point`, towards another point (which it passes through as well) or along a direction
        (u, v). Constrain it like a line (`vertical(axis)`, `point_on(p, axis)`, `angle`, ...).
        A dimension from a point or a line to it, placed across it, is a diameter (in the app;
        here `diameter_about(point, axis, value)`), and `Part.revolve` with no axis given turns
        the sketch about it (it is a datum axis of the sketch's: `datum(f"{sketch.id}/{name}")`)."""
        name = self._name(name, "axis")
        towards = [float(towards[0]), float(towards[1])] if isinstance(towards, (tuple, list)) else towards
        self._rotation_axes[name] = {"through": point, "towards": towards}
        return name

    def circle(self, center: str, radius: float, name: str | None = None, construction: bool = False) -> str:
        name = self._name(name, "c")
        self._circles[name] = {"center": center, "radius": float(radius)}
        if construction:
            self._construction.append(name)
        return name

    def arc(self, center: str, start: str, end: str, name: str | None = None, cw: bool = False) -> str:
        """An arc about `center`, counter-clockwise from `start` to `end` (clockwise with cw)."""
        name = self._name(name, "a")
        self._arcs[name] = {"center": center, "start": start, "end": end, "cw": cw}
        return name

    def polyline(self, coords, closed: bool = True, prefix: str | None = None) -> Shape:
        """Lines through the given (u, v) places, end to end; closed back to the first."""
        prefix = prefix or self._name(None, "poly")
        points = [self.point(u, v, f"{prefix}_p{k}") for k, (u, v) in enumerate(coords)]
        pairs = list(zip(points, points[1:] + points[:1])) if closed else list(zip(points, points[1:]))
        lines = [self.line(a, b, f"{prefix}_l{k}") for k, (a, b) in enumerate(pairs)]
        return Shape(points, lines)

    def rect(self, prefix: str, u: float, v: float, width: float, height: float) -> Shape:
        """A rectangle with its sides along the axes: lines bottom, right, top, left; points
        counter-clockwise from the lower left. Its size and place still need dimensions."""
        shape = self.polyline([(u, v), (u + width, v), (u + width, v + height), (u, v + height)], True, prefix)
        b, r, t, l = shape.curves
        self.horizontal(b), self.horizontal(t), self.vertical(r), self.vertical(l)
        return shape

    def slot(self, prefix: str, u: float, v: float, between: float, width: float, upright: bool = False) -> Shape:
        """A slot (obround) starting about (u, v): two half circles `between` apart joined by two
        lines, along u (or along v with upright). Points: the two centres, then the four
        ends; curves: the two lines, then the two arcs. Its width and the distance between
        its centres are dimensioned already; its place and direction are yours to say
        (e.g. `horizontal(c0, c1)` and distances to the first centre)."""
        r = width / 2
        du, dv = (0, 1) if upright else (1, 0)
        nu, nv = -dv, du  # to the left of its direction
        c0 = self.point(u, v, f"{prefix}_c0")
        c1 = self.point(u + du * between, v + dv * between, f"{prefix}_c1")
        a0 = self.point(u + nu * r, v + nv * r, f"{prefix}_a0")
        b0 = self.point(u - nu * r, v - nv * r, f"{prefix}_b0")
        a1 = self.point(u + du * between + nu * r, v + dv * between + nv * r, f"{prefix}_a1")
        b1 = self.point(u + du * between - nu * r, v + dv * between - nv * r, f"{prefix}_b1")
        la = self.line(a0, a1, f"{prefix}_la")
        lb = self.line(b0, b1, f"{prefix}_lb")
        arc0 = self.arc(c0, a0, b0, f"{prefix}_arc0")  # around the start, away from the end
        arc1 = self.arc(c1, b1, a1, f"{prefix}_arc1")
        self.tangent(la, arc0), self.tangent(la, arc1), self.tangent(lb, arc0), self.tangent(lb, arc1)
        self.distance(c0, c1, between)
        self.radius(arc0, r)
        self.equal(arc0, arc1)
        return Shape([c0, c1, a0, b0, a1, b1], [la, lb, arc0, arc1])

    def polygon(self, prefix: str, center: str, sides: int, across_flats: float, guess=(0.0, 0.0)) -> Shape:
        """A regular polygon about the point `center` (e.g. a hexagon, `sides=6`), dimensioned
        across its flats; it can still turn about its centre until one side is given a
        direction (`horizontal(shape.curves[0])`). `guess` is where the centre is, about."""
        radius = across_flats / 2 / math.cos(math.pi / sides)
        circle = self.circle(center, radius, f"{prefix}_circle", construction=True)
        start = -math.pi / 2 - math.pi / sides
        coords = [(guess[0] + radius * math.cos(start + 2 * math.pi * k / sides),
                   guess[1] + radius * math.sin(start + 2 * math.pi * k / sides)) for k in range(sides)]
        shape = self.polyline(coords, True, prefix)
        for point in shape.points:
            self.point_on(point, circle)
        for side in shape.curves[1:]:
            self.equal(shape.curves[0], side)
        self.point_line(center, shape.curves[0], across_flats / 2)
        return shape

    # constraints
    def _c(self, *what):
        self._constraints.append(list(what))

    def coincident(self, a, b): self._c("coincident", a, b)
    def point_on(self, point, curve): self._c("point_on", point, curve)
    def horizontal(self, a, b=None): self._c("horizontal", a) if b is None else self._c("horizontal", a, b)
    def vertical(self, a, b=None): self._c("vertical", a) if b is None else self._c("vertical", a, b)
    def parallel(self, a, b): self._c("parallel", a, b)
    def perpendicular(self, a, b): self._c("perpendicular", a, b)
    def collinear(self, a, b): self._c("collinear", a, b)
    def tangent(self, a, b, internal: bool = False): self._c("tangent", a, b, internal)
    def equal(self, a, b): self._c("equal", a, b)
    def midpoint(self, point, line): self._c("midpoint", point, line)
    def symmetric(self, a, b, line): self._c("symmetric", a, b, line)
    def construction(self, *curves): self._construction.extend(curves)

    def fix(self, *points): self._fixed.extend(points)

    # dimensions (driving). Each takes show_in="<view id>" to say which drawing view carries
    # it (over the sketch's own show_in), or on_drawing=False to keep just this one off the sheet.
    # reference=True makes it a driven dimension (a reference dimension): it holds nothing (the
    # sketch must be constrained without it), reports what the rest leaves it to be, and is drawn
    # in brackets. A driving dimension that over-defines the sketch is reported (redundant /
    # conflicting) in the build report; the app adds such a one as a driven one.
    def _d(self, what, show_in=None, on_drawing=None, reference=None, at=None, decimals=None, id=None):
        how = {k: v for k, v in (("show_in", show_in), ("on_drawing", on_drawing), ("reference", reference),
                                 ("at", list(at) if at is not None else None), ("decimals", decimals),
                                 ("id", id)) if v is not None}
        if how:
            self._dimension_options[str(len(self._constraints))] = how
        self._c(*what)

    def distance(self, a, b, value, far: bool = False, **how):
        """The distance between two points; or measured to the side of what is round: from a
        point or a line to a circle or arc (its near side; far=True its far one — a slot's whole
        length from its end line), or the gap between two circles or arcs (a wall, a clearance)."""
        self._d(("distance_far" if far else "distance", a, b, float(value)), **how)

    def diameter_about(self, point, line, value, **how):
        """Twice the distance of a point from a line, shown as Ø: the diameter, about that line
        as axis (a `rotation_axis`, "u", "v" or any line), of what the point sweeps — how a
        revolve's profile is dimensioned."""
        self._d(("diameter_about", point, line, float(value)), **how)
    def distance_u(self, a, b, value, **how): self._d(("distance_u", a, b, float(value)), **how)
    def distance_v(self, a, b, value, **how): self._d(("distance_v", a, b, float(value)), **how)
    def point_line(self, point, line, value, **how): self._d(("point_line", point, line, float(value)), **how)
    def radius(self, curve, value, **how): self._d(("radius", curve, float(value)), **how)
    def diameter(self, curve, value, **how): self._d(("diameter", curve, float(value)), **how)
    def angle(self, a, b, degrees, **how): self._d(("angle", a, b, float(degrees)), **how)

    def _step(self) -> dict:
        return {"sketch": {
            "id": self.id, "plane": self.plane, "points": self._points, "lines": self._lines,
            "circles": self._circles, "arcs": self._arcs, "construction": self._construction,
            "rotation_axes": self._rotation_axes,
            "fixed": self._fixed, "dots": self._dots, "constraints": self._constraints,
            "dimension_options": self._dimension_options, "project": self._projected,
        }}


# --- parts -----------------------------------------------------------------------------------

class Part:
    """A part: its steps in order. unit: "Millimetre" (default), "Centimetre", "Metre", "Inch"."""

    def __init__(self, unit: str | None = None, material: str | None = None, file: str | None = None,
                 title: str | None = None, sectioned: bool = True):
        """title: what the part calls itself (its property "Title"): what its title block and an
        assembly's parts list say of it ("WOOD BOX"), else its file's name. sectioned=False (a
        shaft, a pin, a key): a section of an assembly through it draws it whole, not hatched
        (ISO 128-50; catalogue screws are so by themselves)."""
        self.unit = unit
        self.material = material
        self.title = title
        self.sectioned = sectioned
        # The file it is built as when an assembly places it (see `add_part`).
        self.file = file
        self._placed_parts: dict = {}
        self.reports: dict = {}
        self._steps: list = []
        self._ids: dict = {}
        self._dimension_views: dict = {}
        self._dimension_at: dict = {}
        self._dimension_decimals: dict = {}
        self._dimension_count: dict = {}
        self._off_sheet: list = []
        self._expect = None
        self._built = None

    def expect(self, original) -> None:
        """Declares what the original drawing has (`original(...)`, see workpiece_check): the build
        then prints a scorecard against it (views, numbers, notes, mass, problems) and lints the
        script's typed numbers against the declared ones."""
        self._expect = original
        workpiece_check._declared.append(original)

    def _fresh(self, prefix: str) -> str:
        self._ids[prefix] = self._ids.get(prefix, 0) + 1
        return f"{prefix}{self._ids[prefix]}"

    def sketch(self, plane="XY", id: str | None = None, on_drawing: bool = True, show_in: str | None = None) -> Sketch:
        """A sketch on "XY", "YZ" or "ZX" of the origin, on a datum plane (`datum(id)`) or on
        a flat face (`topo(name)`) — a plane is said so wherever a step takes one. Its driving dimensions go on the drawing: in the view
        `show_in` (a view's id) if that view shows them true, else the first that does; with
        on_drawing=False (a helper or cutter sketch whose numbers are not the drawing's) not
        at all."""
        sketch = Sketch(id or self._fresh("sketch"), plane, on_drawing, show_in)
        self._steps.append(sketch)
        return sketch

    def on_sheet(self, step: str, show_in: str | None = None, on_drawing: bool = True, at=None,
                 decimals: int | None = None, count=None) -> None:
        """Where the dimensions a step makes of its own (an extrude's distance, a revolve's
        angle, a hole's diameter and depth) go on the drawing: in the view `show_in`, or with
        on_drawing=False nowhere (a number the original drawing does not show, or shows once
        already through a sketch). Of an assembly, `step` may be a placed part's own:
        `placed("box", "base_sk/width")` (or "box/base_sk/width") puts that part's dimension on
        the assembly's sheet, where the part is placed. Of a hole's callout (`"bore/callout"`),
        count=6 says it is of six holes (`6×`, or `(6) PLACES` on a sheet with places=True)
        whatever the model says, count=False says no count; by default the count is the hole's
        places times what patterns and mirror images of the hole made after it repeat it."""
        if isinstance(step, dict):
            names = []
            while isinstance(step, dict) and "Placed" in step:
                names.append(step["Placed"]["instance"])
                step = step["Placed"]["source"]
            step = "/".join(names + [step])
        if not on_drawing:
            self._off_sheet.append(step)
        elif show_in:
            self._dimension_views[step] = show_in
        if at is not None:
            # (Where its text stands: millimetres on the sheet from the view's middle.)
            self._dimension_at[step] = list(at)
        if decimals is not None:
            self._dimension_decimals[step] = decimals
        if count is not None:
            self._dimension_count[step] = 0 if count is False else int(count)

    def op(self, _kind: str, _id: str | None = None, /, id: str | None = None, **args) -> str:
        """Any step by its kind (run `workpiece kinds` for all of them and their arguments);
        arguments not given are as by default. `p.op("Hole", "h1", kind="Counterbore", ...)`:
        the kind of step comes first, by position; `id=` names the step; everything else named
        is an argument of the step."""
        _id = _id or id or self._fresh(_kind.lower())
        if args.get("areas") is not None:
            args["areas"] = [_area(a) for a in args["areas"]]
        self._steps.append({"op": _kind, "id": _id, "args": args})
        return _id

    # the common features
    def extrude(self, sketch, distance: float = 10.0, combine: str = "Join", symmetric: bool = False,
                through: bool = False, flip: bool = False, id: str | None = None, **more) -> str:
        """Extrude a sketch along its plane's normal (flip: the other way). combine: "NewBody",
        "Join", "Cut", "Intersect". symmetric: `distance` in all, half each way. through:
        through everything that way; with symmetric as well, through everything both ways.
        areas: which closed areas of the sketch, each by a (u, v) point inside it
        (`areas=[(10, 5), (40, 5)]`); left out, the whole sketch (areas inside others are holes).
        The same `areas` goes for revolve and the other steps that take a sketch's areas."""
        extent = ("ThroughBoth" if symmetric else "ThroughAll") if through else "Symmetric" if symmetric else "Distance"
        return self.op("Extrude", id, sketch=profile(sketch), distance=float(distance), combine=combine,
                       extent=extent, flip=flip, **more)

    def cut(self, sketch, distance: float = 10.0, through: bool = False, flip: bool = False,
            symmetric: bool = False, **more) -> str:
        """Cut a sketch out (an extrude with combine "Cut"). A cut goes into the body: from
        a sketch on the body's outside it turns towards the body by itself. through=True
        cuts through everything that way; with symmetric=True, both ways."""
        return self.extrude(sketch, distance, "Cut", through=through, flip=flip, symmetric=symmetric, **more)

    def revolve(self, sketch, about=None, angle: float = 360.0, combine: str = "Join", id: str | None = None, **more) -> str:
        """Revolve a sketch about an axis. `about`: the name of a rotation axis drawn in the sketch
        (`Sketch.rotation_axis`); "u" or "v", the sketch plane's own axes (drawn into the sketch as
        a rotation axis along that one); or a reference to any line — `axis("Z")`, `datum(axis_id)`,
        `topo(straight_edge)`. Left out: the sketch's one rotation axis if it has one, else "u"."""
        if isinstance(sketch, Sketch):
            drawn = list(sketch._rotation_axes)
            if about is None:
                if len(drawn) > 1:
                    raise ValueError(f"sketch {sketch.id!r} has rotation axes {drawn}: say which with about=")
                about = drawn[0] if drawn else "u"
            if about in ("u", "v"):
                name = f"{about}_turn"
                if name not in sketch._rotation_axes:
                    sketch.rotation_axis(sketch.origin, (1.0, 0.0) if about == "u" else (0.0, 1.0), name)
                    sketch.parallel(name, about)
                about = name
            if isinstance(about, str):
                if about not in sketch._rotation_axes:
                    raise ValueError(f"about={about!r}: a rotation axis of sketch {sketch.id!r} "
                                     f"({list(sketch._rotation_axes)}), \"u\", \"v\" or a reference such as axis(\"Z\")")
                about = {"Datum": f"{sketch.id}/{about}"}
        if not isinstance(about, dict):
            raise ValueError("about= is a reference (axis(\"Z\"), datum(id), topo(edge)) for a sketch given by its id")
        return self.op("Revolve", id, sketch=profile(sketch), axis=about, angle=float(angle), combine=combine, **more)

    def hole(self, sketch=None, diameter: float = 6.0, depth: float | None = None, kind: str = "Simple",
             id: str | None = None, up_to=None, head_up_to=None, next: bool = False, on=None, at=None,
             off=None, around=None, radius: float | None = None, angle: float = 0.0, **more) -> str:
        """Holes, drilled into the body. Where — one of four placements:
        `sketch` (its dots, `sketch.dot(...)`: a hole at each, on the sketch's plane or on `on`);
        `on=topo(face), at=(x, y)` (a point of the face's plane, in its own x and y from its origin);
        `on=topo(face), off=[(topo(side_or_edge), 10), (topo(other), 15)]` (a distance from each of
        two planes or straight edges, inward from a face of the body or an edge of the face);
        `on=topo(face), around=axis("Z"), radius=20, angle=30` (from an axis, at an angle from the
        face's x). How far it goes: `depth` (blind); no depth, through everything that way;
        `next=True`, through the wall it starts in; `up_to=topo(face)` or `up_to=datum(plane_id)`,
        to that face's or plane's plane, and `offset=` further in (negative: short of it). kind:
        "Simple", "Counterbore" (head=diameter, head_depth=... or `head_up_to=topo(face)` /
        `datum(plane_id)` for its floor, `head_offset=`), "Countersink" (head=diameter,
        head_angle=...), "Tapped". `hexagon` only sizes a standard counterbore for a hexagon head;
        a hexagonal pocket is a cut of `sketch.polygon(...)`. The drawing's callout gives a depth
        that comes of a face in brackets, as a reference."""
        args = dict(diameter=float(diameter), kind=kind, **more)
        if on is not None:
            args.update(face=on)
        if sketch is not None:
            args.update(place="Sketch", sketch=profile(sketch))
        elif off is not None:
            (first, d1), (second, d2) = off
            args.update(place="Offsets", first=first, first_offset=float(d1), second=second,
                        second_offset=float(d2))
        elif around is not None:
            args.update(place="Polar", axis=around, radius=float(radius), angle=float(angle))
        elif at is not None:
            args.update(place="Point", at=[float(at[0]), float(at[1])])
        else:
            raise ValueError("a hole needs a sketch, or on= with at=, off= or around=")
        if up_to is not None:
            args.update(extent="ToFace", up_to=up_to)
        elif next:
            args.update(extent="ToNext")
        elif depth is None:
            args.update(extent="ThroughAll")
        else:
            args.update(extent="Distance", depth=float(depth))
        if head_up_to is not None:
            args.update(head_extent="ToFace", head_up_to=head_up_to)
        return self.op("Hole", id, **args)

    @staticmethod
    def _spread(count, about, angle, spacing, along, points, fill, grid, across, turned, margin, sides) -> dict:
        """Where the copies of a pattern go, as the step's arguments."""
        if about is not None:
            return dict(along="Circle", axis=about, count=int(count), angle=float(angle))
        if points is not None:
            return dict(along="Points", sketch=profile(points))
        if fill is not None:
            out = dict(along="Fill", face=fill, grid=grid, spacing=float(spacing), margin=float(margin),
                       turned=float(turned), axis=along)
            if across is not None:
                out["across"] = float(across)
            if sides is not None:
                out["sides"] = int(sides)
            return out
        return dict(along="Line", axis=along, count=int(count), spacing=float(spacing))

    def feature_pattern(self, faces, count: int = 2, about=None, angle: float = 360.0, spacing: float | None = None,
                        along=None, points=None, fill=None, grid: str = "Square", across: float | None = None,
                        turned: float = 0.0, margin: float = 0.0, sides: int | None = None,
                        id: str | None = None, **more) -> str:
        """Repeat features — told by one face each of what the feature made (a hole's wall, a
        boss's side). Around an axis: `about=axis("Z")`, `count` in all over `angle` (a full
        turn is shared out evenly; less than a full turn has the first and the last at its
        ends, so 3 over 180 are 90 apart). Along a line: `along=axis("X")`, `spacing` apart. At
        the dots of a sketch: `points=sketch` (moved from where the feature is to each dot; none
        at a dot where it is already). Filling a flat face: `fill=topo(face)`, `grid=` "Square",
        "Rectangular", "Staggered", "Hexagonal", "Rings" or "Polygon", `spacing` (and `across`,
        the rows' or rings' spacing, `sides` of a polygon), `turned` degrees from the face's x
        (or from `along`), `margin` kept in from its edges; what would cross it is left out."""
        where = self._spread(count, about, angle, spacing, along, points, fill, grid, across, turned, margin, sides)
        return self.op("Pattern", id, of="Features", features=[topo(f) for f in faces], **where, **more)

    def feature_mirror(self, faces, plane, id: str | None = None, **more) -> str:
        """Mirror features — told by one face each of what the feature made — in a plane: "X",
        "Y", "Z" (the origin's plane square to that axis), a datum plane (`datum(id)`) or a
        flat face (`topo(face)`). Steps between the picked ones are mirrored with them."""
        ref = axis(plane) if isinstance(plane, str) and len(plane) == 1 else plane
        return self.op("Mirror", id, of="Features", features=[topo(f) for f in faces], plane=ref, **more)

    def pattern(self, body, count: int = 2, about=None, angle: float = 360.0, spacing: float | None = None,
                along=None, combine: str = "NewBody", points=None, fill=None, grid: str = "Square",
                across: float | None = None, turned: float = 0.0, margin: float = 0.0, sides: int | None = None,
                id: str | None = None, **more) -> str:
        """Repeat whole bodies (a name, or a list of names) — where they go as for
        `feature_pattern`; as bodies of their own ("NewBody"), joined into one ("Join"), or cut
        out of `target=topo(body)` ("Cut")."""
        bodies = [body] if isinstance(body, str) else list(body)
        where = self._spread(count, about, angle, spacing, along, points, fill, grid, across, turned, margin, sides)
        return self.op("Pattern", id, of="Bodies", bodies=[topo(b) for b in bodies], combine=combine,
                       **where, **more)

    def mirror(self, body, plane, join: bool = False, id: str | None = None, **more) -> str:
        """Mirror whole bodies (a name, or a list of names) in a plane, as for `feature_mirror`:
        a body of its own, or (`join=True`) one with the body."""
        bodies = [body] if isinstance(body, str) else list(body)
        ref = axis(plane) if isinstance(plane, str) and len(plane) == 1 else plane
        return self.op("Mirror", id, of="Bodies", bodies=[topo(b) for b in bodies], plane=ref,
                       combine="Join" if join else "NewBody", **more)

    def sweep(self, section, path, combine: str = "Join", id: str | None = None, **more) -> str:
        """Sweep a section (a sketch, or `topo(face)` for a face's outline) along a path: a
        sketch's curves, or a curve body `topo(name)`. `twist=` degrees, `scale=` at its end."""
        return self.op("SweptLoft", id, sections=[_section(section)],
                       rails=[{"of": _curve(path), "role": "Path"}], combine=combine, **more)

    def loft(self, sections, guides=(), start: str = "Free", end: str = "Free", start_face=None,
             end_face=None, surface: bool = False, combine: str = "Join", id: str | None = None,
             **more) -> str:
        """Loft through `sections` in order: sketches, faces (`topo(face)`: its outline) or, for
        `surface=True`, edges and curves left open. `guides`: curves (sketches or curve bodies)
        the loft runs along, each touching every section. How it leaves the first section and
        arrives at the last: "Free", "Normal" (square to the section's plane), "Tangent" (G1)
        or "Curvature" (G2) to a face — `start_face=topo(face)` (none: the section's own face)."""
        secs = [_section(s) for s in sections]
        for k, way, face in ((0, start, start_face), (-1, end, end_face)):
            secs[k]["end"] = way
            if face is not None:
                secs[k]["face"] = face
        return self.op("SweptLoft", id, sections=secs, rails=[{"of": _curve(g), "role": "Guide"} for g in guides],
                       surface=surface, combine=combine, **more)

    def fillet(self, edges, radius: float, id: str | None = None, **more) -> str:
        return self.op("Fillet", id, edges=[topo(e) for e in edges], size=float(radius), **more)

    def chamfer(self, edges, distance: float, id: str | None = None, **more) -> str:
        return self.op("Chamfer", id, edges=[topo(e) for e in edges], distance=float(distance), **more)

    def shell(self, open_faces, thickness: float, id: str | None = None, **more) -> str:
        return self.op("Shell", id, faces=[topo(f) for f in open_faces], thickness=float(thickness), **more)

    def plane(self, base, distance: float, id: str | None = None) -> str:
        """A datum plane parallel to `base` ("X", "Y", "Z" for the origin's plane square to that
        axis, or a reference) a distance along its normal. Sketch on it with `datum(id)`. Made
        from a flat face it has the face's own axes: normal out of the body, "up" up (v = world +z
        projected onto the plane, u = v × normal: a face looking along −x has u = −y, v = z; a top
        face u = x, v = y; a bottom face u = x, v = −y)."""
        ref = axis(base) if isinstance(base, str) and len(base) == 1 else base
        return self.op("AddPlane", id, mode="Offset", distance=float(distance), references=[ref])

    def drawing(self, views=None, scale=(1, 1), format: str = "A3", upright: bool = False,
                projection: str = "ThirdAngle", dimensions: bool = True, id: str | None = None,
                notes=None, decimals: int | None = None, leading_zero: bool | None = None,
                comma: bool | None = None, tangent_edges: str = "Thin", centre_lines="all",
                line_width: float | None = None, parts_list: bool = False, balloons=None,
                lettering: float = 3.5, thru: bool | None = None, trim_zeros: bool = False,
                aligned_leaders: bool = False, places: bool = False,
                reference_depths: bool = False) -> str:
        """A drawing sheet of the model as the steps before it leave it. `views` is a list made
        with `view(...)`, `projected(...)`, `section_view(...)` (default: front, a view to its
        right, one above it, and an isometric one). With `dimensions`, every driving dimension
        of the model's sketches (and every Dimension step) is put on the sheet in a view that
        shows it true. Build with render=True to get it as PDF, SVG and PNG.
        How numbers are written: `decimals` fixes how many every number has (None: as many as
        it needs, at most four), leading_zero=False writes ".750", comma=False a decimal point.
        An inch part (`Part(unit="Inch")`) writes ".750" style with three decimals unless said
        otherwise; any dimension can say its own `decimals=`.
        tangent_edges: how smooth edges (where a fillet runs into the face it rounds) are drawn:
        "Thin" (default), "Hidden" (not at all) or "Wide" (as any edge).
        centre_lines: the centre lines (ISO 128-2, long-dashed dotted narrow) the sheet draws —
        "all" (of everything round: bores, bosses, turned bodies; default), "holes" (of holes
        only), "none", or a list of names, each drawn in every view that can show it: a datum
        (`p.plane(...)`'s id, "origin"; a frame's axes "origin/x", "origin/z": the trace of a
        plane seen edge on, an axis, a point as a small cross), a hole or round feature (the id of
        the step that made it, or a face's name: its centre line, a centre mark seen end on), or
        ("pitch", id) for the pitch circle of three or more like holes about one middle, a sketch's
        id for it as a path's centre line (a sweep's path: drawn by "all" by itself), an Unfold
        step's id for the bend lines of its flat pattern (each `id/bend1`, ... by itself); the list
        may hold "holes" or "all" as well. Or, for one line, a dict: {"of": name, "view": id,
        "hide": True (take a hole's centre line off), "ends": (start, end) (mm longer, or shorter if
        negative, past the outline), "gaps": [(from, to), ...] (mm along it from its start: broken
        there), "symmetry": True (ISO 128-2 symmetry marks: two short parallel lines across each
        end), "label": "CL SYM" (words on it: by itself at the end where they stand clear of what is
        written; "label_at": "start" | "end" | mm along it from its start). "of" may name the
        origin's planes "XY", "YZ", "ZX": their traces (a plane of symmetry). line_width: the wide lines' width in mm from ISO 128-2's
        series (0.35, 0.5, 0.7, ...; narrow ones half of it); default 0.5 on A4–A2, 0.7 on A1/A0.
        Of an assembly: parts_list=True puts a parts list (ISO 7573) over the title block — item,
        quantity, name (the placed part's `Part(title=...)`, else its file's name), material; a
        part placed several times is one item. A list says the columns and their order:
        parts_list=["item", "qty", "name", "material", "density"]. balloons="auto" gives each item a balloon (ISO
        6433: its number in a circle, on a leader ending in a dot on the part) in the first view
        that shows it, clear of the rest; or a list of `balloon(part, view, at)` (part: the id it
        is placed under) puts them by hand — "auto" in that list balloons the rest by itself.
        lettering: the height of dimensions' and notes' lettering in mm (ISO 3098: 2.5, 3.5, 5, 7);
        views' names, section and detail letters and balloons' numbers are the next size up.
        thru: hole callouts say "THRU" of a hole through (`Ø6 THRU`; default for inch parts, as
        ASME drawings do); else, as ISO 129-1 has it, a callout with no depth is through.
        trim_zeros: with `decimals` (or an inch part's three), a number has no more decimals than
        it needs (.5, 1.125, 2.75) — every dimension and callout; a dimension's own `decimals=`
        still says exactly how many it has.
        A radius or diameter on a leader (its text outside the round) stands level on a short
        shoulder at the leader's end (ISO 129-1); aligned_leaders=True writes it along the leader.
        places=True writes how many holes alike a hole's callout is of as "(6) PLACES" on a line
        after it, as some drawings do, instead of "6×" before it (ISO 129-1). reference_depths=True
        writes a hole's depth that follows from a face (`up_to=`, `head_up_to=`) in its callout as a
        reference, "↧(50)"; by default such a depth is left out (it is dimensioned elsewhere)."""
        inch = self.unit == "Inch"
        numbers = {"decimals": decimals if decimals is not None else (3 if inch else None),
                   "bare": (not leading_zero) if leading_zero is not None else inch,
                   "point": (not comma) if comma is not None else inch, "trim": bool(trim_zeros)}
        w, h = {"A4": (297, 210), "A3": (420, 297), "A2": (594, 420), "A1": (841, 594), "A0": (1189, 841)}[format]
        if upright:
            w, h = h, w
        if views is None:
            views = [view("front", "Front", (w * 0.3, h * 0.45)), projected("right", "front", "Right"),
                     projected("top", "front", "Above"), view("iso", "Isometric", (w * 0.75, h * 0.75))]
        # A projected view stands in line with the view it is of, a distance from it.
        at = {v["id"]: v["at"] for v in views}
        step = {"Right": (1, 0), "Left": (-1, 0), "Above": (0, 1), "Below": (0, -1)}
        for v in views:
            if v["at"] is None:
                kind = v["kind"]["Projected"]
                x, y = at[kind["of"]]
                dx, dy = step[kind["beside"]]
                d = v.pop("distance")
                v["at"] = at[v["id"]] = [x + dx * d, y + dy * d]
        id = id or self._fresh("drawing")
        # The camera views of views given by a direction: steps of their own, before the sheet.
        for v in views:
            if "_camera" in v:
                self._steps.append({"op": "CameraView", "id": v["kind"]["Named"]["view"],
                                    "args": {"looking": "Current", "view": v.pop("_camera")}})
        # (A flat pattern's id names its bend lines, `flat/bend1`, ...: datums too.)
        datums = {"origin"} | {s["id"] for s in self._steps if isinstance(s, dict)
                               and s.get("op") in ("AddPlane", "AddAxis", "AddFrame", "Unfold")}
        centres, lines = "None", []
        sketch_ids = {s.id for s in self._steps if isinstance(s, Sketch)}
        def of(name):
            if isinstance(name, (tuple, list)) and name[0] == "pitch":
                return {"Pitch": name[1]}
            # (A sweep's path sketch: its centre line.)
            if name in sketch_ids:
                return {"Path": name}
            # (The origin's planes by their names: their traces.)
            if name in ("XY", "YZ", "ZX"):
                return {"Datum": "origin/" + name.lower()}
            return {"Datum": name} if name.split("/")[0] in datums else {"Axis": name}
        for name in ([centre_lines] if isinstance(centre_lines, str) else list(centre_lines)):
            if isinstance(name, str) and name in ("all", "holes", "none"):
                centres = name.capitalize()
            elif isinstance(name, dict):
                # One line, said how it is drawn.
                line = {"of": of(name["of"])}
                if name.get("view") is not None:
                    line["view"] = name["view"]
                if name.get("hide"):
                    line["hidden"] = True
                if name.get("ends") is not None:
                    line["ends"] = [float(x) for x in name["ends"]]
                if name.get("gaps"):
                    line["gaps"] = [[float(a), float(b)] for a, b in name["gaps"]]
                if name.get("symmetry"):
                    line["symmetry"] = True
                if name.get("label"):
                    line["label"] = name["label"]
                if name.get("label_at") is not None:
                    at = name["label_at"]
                    line["label_at"] = at if isinstance(at, str) else float(at)
                lines.append(line)
            else:
                lines.append({"of": of(name)})
        # With every centre line, the paths sweeps run along (a bent pipe's centre line).
        if centres == "All":
            for step in self._steps:
                rails = step["args"].get("rails", []) if isinstance(step, dict) and step.get("op") == "SweptLoft" else []
                path = next((r["of"] for r in rails if r.get("role", "Path") == "Path"), None)
                if isinstance(path, dict) and "Profile" in path and not any(
                        l["of"] == {"Path": path["Profile"]} for l in lines):
                    lines.append({"of": {"Path": path["Profile"]}})
        sketches = [s for s in self._steps if isinstance(s, Sketch)]
        shown = {s.id: s.show_in for s in sketches if s.show_in}
        shown.update(self._dimension_views)
        self._steps.append({"op": "Drawing", "id": id, "place_notes": dimensions,
                            "hide_dimensions_of": [s.id for s in sketches if not s.on_drawing] + self._off_sheet,
                            "dimension_at": dict(self._dimension_at),
                            "dimension_decimals": dict(self._dimension_decimals),
                            "dimension_count": dict(self._dimension_count),
                            "dimension_views": shown, "args": {
            "name": id, "format": format, "upright": upright, "scale": list(scale),
            "projection": projection, "views": views, "notes": [], "own": [], "numbers": numbers,
            "words": list(notes or []), "tangent_edges": tangent_edges, "centres": centres,
            "lines": lines, "drafting": {"wide": line_width, "letter": float(lettering)},
            "parts_list": bool(parts_list),
            "parts_columns": [c.capitalize() for c in parts_list] if isinstance(parts_list, (list, tuple)) else [],
            "auto_balloons": balloons == "auto" or (isinstance(balloons, list) and "auto" in balloons),
            "balloons": [b for b in (balloons if isinstance(balloons, list) else []) if b != "auto"],
            "thru": inch if thru is None else bool(thru), "aligned_leaders": bool(aligned_leaders),
            "places": bool(places), "reference_depths": bool(reference_depths)}})
        return id

    def dimension(self, *of: str, show_in: str | None = None, id: str | None = None, kind: str = "Auto",
                  reference: bool = False, supplement: bool = False, at=None, far: bool = False,
                  centres: bool = False, same_side: bool = False, along=None, bend: str = "Own",
                  plain: bool = False, decimals: int | None = None, **more_args) -> str:
        """A dimension on the model itself, for a number no sketch carries (an extrude's depth,
        a plane's offset, a hole's depth): between two faces or edges by name (their distance),
        or of one (an edge's length, a round face's diameter). It goes on the drawing like a
        sketch's dimensions; `show_in` names the view. kind: "Auto", "Distance", "Diameter",
        "Radius" (of a round face), "Angle". Two round faces about one axis give how far the one
        is off the other (an "offset"). An angle between two flat faces is the one between the
        walls themselves, through where they meet (a chamfer face and its wall: 135°);
        supplement=True gives what is left of a half turn (45°: how far the chamfer leans off
        the wall). reference=True writes it in brackets, as a number that follows from others.

        Round things: an arc or circle *edge* is said by its radius (a whole circle by its
        diameter), and so is any face that bends as a circle one way across — a fillet around
        a rim, a ball, what a sweep makes along an arc (kind="Radius"/"Diameter" to choose).
        Between two round things not about one axis the distance is their gap (their near
        sides); far=True their far sides, centres=True their middles (two balls: always their
        centres). Between a round thing and a flat face, a straight edge or a datum
        (`datum(id)`, `axis("Z")`) likewise: to its near side, far side, or middle.
        same_side=True: from the one's side to the other's side that faces the same way (how
        far a rounded end stands out past another: extreme to extreme).
        along=axis("Z"): measured along that direction only — from how far the one reaches
        that way to how far the other does (near sides; far, same_side, centres as above) —
        for things that do not lie square to each other (a bend's bottom below a wire's top).
        bend: of a face that bends two ways (a swept bend, a fillet around a rim), which
        radius kind="Radius"/"Diameter" gives: "Own" (the tube's, default), "Middle" (the
        bend's, along the tube's middle), "Inside" or "Outside" (the bend's on its inside or
        outside: R6 and R12 for a Ø6 wire bent at R9). plain=True
        writes a radius as the bare number ("(3)", with reference=True). `at=(x, y)` says where
        its text stands, in mm from its view's middle; `decimals=` how many it is written with."""
        more = {"tolerance": "Reference"} if reference else {}
        more.update(more_args, far=far, centres=centres, same_side=same_side, bend=bend, plain=plain)
        if along is not None:
            more["along"] = along
        refs = [name if isinstance(name, dict) else topo(name) for name in of]
        made = self.op("Dimension", id, of=refs, kind=kind, supplement=supplement, **more)
        if show_in:
            self._dimension_views[made] = show_in
        if at is not None:
            self._dimension_at[made] = list(at)
        if decimals is not None:
            self._dimension_decimals[made] = decimals
        return made

    def add_part(self, part, mates=None, fixed: bool | None = None, at=None, euler=None, bodies=None,
                 variant: str | None = None, id: str | None = None) -> str:
        """Places a part in this one: an assembly. `part` is a .wpc file beside this one
        (built already), or a `Part` of the script — given a `file` name (`Part(file=...)`), it is
        built into this one's folder first when this one is built, once however often it is
        placed. It is held by its `mates` (a list of `mate(onto, with_, constraint, ...)`): each a
        reference on what is here already, one of its own and the constraint between them;
        "Fastened" with the two origins puts it where it was modelled. Without mates it is fixed
        where it is put (`at=(x, y, z)`, `euler=(x, y, z)` degrees about the fixed x, y, z axes,
        in turn, say where its origin goes); fixed=False leaves it free to drag, fixed=True keeps
        a mated part where it is (its mates then move what they join to it). bodies=[names] brings
        only those of its bodies — not for parts you model: each item of a parts list is a
        standalone part (see README). Returns the step's id, which also names the placed part
        (`placed(id, ...)`, `anchor(..., instance=id)`, `report.placed_part(id)`); its joints are
        named after it (the second mate's `id/2`)."""
        if isinstance(part, Part):
            if not part.file:
                raise ValueError("a Part placed by an assembly needs a file name: Part(file=\"pin.wpc\")")
            self._placed_parts[part.file] = part
            part = part.file
        args = {"part": part, "hold": _hold(mates, fixed), "bodies": list(bodies or [])}
        if variant is not None:
            args["variant"] = variant
        made = self.op("AddPart", id or self._fresh(os.path.splitext(os.path.basename(part))[0]), **args)
        self._pose(at, euler)
        return made

    def standard_part(self, kind: str, size: str, length: float | None = None, mates=None,
                      fixed: bool | None = None, at=None, euler=None, id: str | None = None) -> str:
        """Places a part out of the standards: kind "HexScrew" (ISO 4017), "SocketScrew" (ISO 4762),
        "HexNut" (ISO 4032), "Washer" (ISO 7089), "DowelPin" (ISO 8734), "Bearing" (ISO 15); size
        "M3".."M16" (a pin's diameter "3".."16", a bearing "608", "6204", ...); a screw's or a
        pin's `length` (one it comes in). Held as `add_part` holds a part: by `mates` — its seat,
        `mate(onto=..., with_=seat(), constraint="Revolute")`, stands it on a face or a hole's rim —
        or fixed `at` a pose. Its bodies are named after the step: "<id>/head/start" is a screw's
        face under the head. Returns the step's id."""
        args = {"kind": kind, "size": size, "hold": _hold(mates, fixed)}
        if length is not None:
            args["length"] = float(length)
        made = self.op("StandardPart", id or self._fresh(kind.lower()), **args)
        self._pose(at, euler)
        return made

    def _pose(self, at, euler) -> None:
        """Where the part the last step placed is put, while no mate holds it."""
        if at is not None or euler is not None:
            self._steps[-1]["pose"] = {"at": [float(x) for x in (at or (0, 0, 0))],
                                      "euler": [float(x) for x in (euler or (0, 0, 0))]}

    def mass_properties(self, id: str | None = None, **more) -> str:
        return self.op("MassProperties", id, **more)

    # building
    def recipe(self) -> dict:
        steps = [s._step() if isinstance(s, Sketch) else s for s in self._steps]
        made = {"unit": self.unit, "material": self.material, "steps": steps}
        properties = {"Title": self.title} if self.title else {}
        if not self.sectioned:
            properties["Sectioned"] = "No"
        if properties:
            made["properties"] = properties
        return made

    def _key(self) -> str:
        """What a build of this part depends on: its recipe and the parts it places."""
        placed = [self._placed_parts[f]._key() for f in sorted(self._placed_parts)]
        return json.dumps([self.recipe(), sorted(self._placed_parts), placed], sort_keys=True)

    def build(self, path: str, render: bool = False, expect=None, pictures: bool | None = None,
              picture: str | None = None, samples: int = 256, size=(1200, 900), seconds: float = 5.0) -> "Report":
        """Writes the part to `path` (a .wpc file), builds it, and returns what came of it.
        With render, the sheets are drawn too: `<path>.views/` gets each drawing as PDF, SVG and
        PNG, and one image per view (`view-<id>.png`), and — `pictures=True`, by default only for
        a part without a drawing — the four pictures iso/front/top/right.png; `sheet.txt` (the
        inventory of the sheets) is written on every build. A build of what an earlier build of
        the same path already built (the same recipe, pictures at least as many) is not done
        again: `report.reused`. `report.seconds` says where the time went.
        The parts of the script it places (`add_part(Part(file=...))`) are built first, into
        the same folder; what came of each is in `self.reports[file]`.
        With a declared original (`p.expect(original(...))` or `expect=`) the report has a
        scorecard (`report.score`), printed by `report.show()`.
        picture="render.png" also traces a picture of it (as the Render step does: from the front
        right and above, on a floor that catches its shadow) into that file: `size` pixels, as many
        as `samples` rays a pixel as `seconds` allow."""
        started = time.time()
        expect = expect or self._expect
        folder = os.path.dirname(os.path.abspath(path))
        for file, part in self._placed_parts.items():
            report = part.build(os.path.join(folder, file), render=render)
            if not report.ok:
                raise RuntimeError(f"{file}, which this part places, did not build: {report.errors}")
            self.reports[file] = report
        full = os.path.abspath(path)
        key = self._key()
        prev = self._built
        if (prev and prev["path"] == full and prev["key"] == key and (prev["render"] or not render) and not picture
                and os.path.exists(full) and os.path.getmtime(full) == prev["mtime"]):
            report = Report(prev["raw"])
            report.reused, report.seconds = True, {"total": time.time() - started}
            report.view_images = prev["views"]
            self._judge(report, expect)
            return report
        with tempfile.TemporaryDirectory() as tmp:
            recipe, report = os.path.join(tmp, "recipe.json"), os.path.join(tmp, "report.json")
            with open(recipe, "w") as f:
                json.dump(self.recipe(), f)
            command = [_binary(), "make", recipe, "-o", path, "--report", report, "--times"]
            if picture:
                command += ["--picture", picture, "--samples", str(int(samples)),
                            "--size", f"{int(size[0])}x{int(size[1])}", "--seconds", str(float(seconds))]
            if render:
                command += ["--render", path + ".views"]
                if pictures is None:
                    pictures = not any(st.get("op") == "Drawing" for st in self.recipe()["steps"] if isinstance(st, dict))
                if not pictures:
                    command += ["--no-pictures"]
            done = subprocess.run(command, capture_output=True, text=True)
            if done.returncode != 0:
                raise RuntimeError(done.stderr.strip() or done.stdout.strip())
            seconds = {"steps": 0.0, "drawings and lint": 0.0, "pictures": 0.0}
            for line in done.stdout.splitlines():
                m = re.match(r"time\s+([\d.]+) s\s+(.*)", line)
                if m:
                    what = m[2]
                    k = "steps" if what.startswith("step ") else "pictures" if what == "pictures" else \
                        "drawings and lint" if what.startswith("lint") else None
                    if k:
                        seconds[k] += float(m[1])
            with open(report) as f:
                raw = json.load(f)
            views = {}
            # A drawing's PDF also as a picture to look at, and one image per view.
            many = len(raw.get("drawings", [])) > 1
            for drawing in raw.get("drawings", []):
                if "pdf" in drawing and workpiece_images.poppler_available():
                    png = drawing["pdf"][:-4]
                    subprocess.run(["pdftoppm", "-png", "-r", "110", "-singlefile", drawing["pdf"], png],
                                   capture_output=True)
                    drawing["png"] = png + ".png"
                    if "inventory" in drawing:
                        for id, file in workpiece_images.view_crops(drawing["pdf"], drawing["inventory"], path + ".views").items():
                            if many:
                                named = os.path.join(path + ".views", f"view-{drawing['id']}-{id}.png")
                                os.replace(file, named)
                                file = named
                            views[f"{drawing['id']}/{id}" if many else id] = file
            report = Report(raw)
            report.view_images = views
            report.seconds = dict(seconds, total=time.time() - started)
            # The sheets as text, beside the pictures, on every build.
            if raw.get("drawings"):
                os.makedirs(path + ".views", exist_ok=True)
                with open(os.path.join(path + ".views", "sheet.txt"), "w") as f:
                    f.write(report.sheet(show=False) + "\n")
            self._judge(report, expect)
            if report.score is not None:
                os.makedirs(path + ".views", exist_ok=True)
                with open(os.path.join(path + ".views", "score.txt"), "w") as f:
                    f.write(report.score["line"] + "\n")
            self._built = {"path": full, "key": key, "render": render, "raw": raw, "views": views,
                           "mtime": os.path.getmtime(full) if os.path.exists(full) else None}
            return report

    def _judge(self, report: "Report", expect) -> None:
        """The scorecard against what the original has, and the lint of the script."""
        steps = self.recipe()["steps"]
        declared = workpiece_check._declared[-1] if workpiece_check._declared else None
        # (What the command line found, kept apart from what is added here on every build.)
        lint = list(report.raw.setdefault("cli_lint", list(report.raw["lint"])))
        if declared is not None:
            lint += workpiece_check.lint_derived(steps, declared)
        main = sys.argv[0] if sys.argv and sys.argv[0].endswith(".py") else None
        if main and os.path.exists(main) and os.path.basename(main) != "workpiece.py":
            with open(main) as f:
                lint += workpiece_check.lint_source(f.read())
        report.score = None
        if expect is not None:
            drawings = [d for d in report.drawings if "inventory" in d]
            inventory = drawings[0]["inventory"] if drawings else {"views": [], "problems": ["the part has no drawing"]}
            report.score = workpiece_check.score(expect, inventory, report.mass_total, self.unit or "Millimetre")
            _, _, missing, extra = report.score["views"]
            lint += [f"the original has a {kind} view the sheet lacks" for kind in missing]
            lint += [f"the sheet has a {kind} view the original does not" for kind in extra]
        report.raw["lint"] = lint
        report.lint = lint


def _xy(p) -> str:
    return "?" if p is None else f"{p[0]:g},{p[1]:g}"


def sheet_text(inv: dict) -> str:
    """One sheet's inventory (a drawing's `inventory` in the report) as compact text: each view
    with its kind, scale, style, place and box, and under it one line per dimension / note /
    centre line / cutting line / balloon; then the parts list and the problems."""
    out = [f"sheet {inv['sheet']}  {inv['size'][0]:g}x{inv['size'][1]:g} mm, {inv['projection']}, scale {inv['scale']}"]
    for v in inv["views"]:
        b = v["box"]
        out.append(f"view {v['id']}  [{v['kind']}] {v['what']}  {v['scale']} {v.get('style') or ''}  "
                   f"at {_xy(v['at'])}  size {_xy(v['size'])}  box {b[0]:g},{b[1]:g}..{b[2]:g},{b[3]:g}")
        for it in v["items"]:
            t = it["type"]
            if t == "dim":
                ends = it.get("ends_off_by")
                e = "ends -" if ends is None else ("ends ok" if not any(ends) else
                                                  "ends OFF " + ",".join(f"{i}:{x:g}mm" for i, x in enumerate(ends) if x))
                ref = " (ref)" if it.get("reference") else ""
                out.append(f"  dim {it['id']}  \"{it['text']}\"{ref}  {it['kind']}  {it['measures']}  {e}  @{_xy(it['at'])}")
            elif t == "words":
                of = f" of {it['of']}" if it.get("of") else ""
                out.append(f"  words \"{it['text']}\"{of}  @{_xy(it['at'])}")
            elif t == "line":
                lab = f" \"{it['label']}\"" if it.get("label") else ""
                out.append(f"  line {it['id']}{lab}  {_xy(it['from'])} -> {_xy(it['to'])}")
            elif t == "cut":
                out.append(f"  cut {it['letter']}-{it['letter']} (view {it['id']})  via " + " ".join(_xy(p) for p in it["path"]))
            elif t == "balloon":
                out.append(f"  balloon item {it['item']} ({it['id']})  @{_xy(it['at'])}")
    for r in inv.get("parts_list", []):
        out.append(f"parts-list item {r['item']}  qty {r['qty']}  {r['name']}  {r['material'] or ''}")
    for pr in inv["problems"]:
        out.append(f"problem: {pr}")
    return "\n".join(out)


@dataclass
class Report:
    """What a build came to."""
    raw: dict
    steps: list = field(init=False)
    lint: list = field(init=False)
    reused: bool = False
    seconds: dict = field(default_factory=dict)
    view_images: dict = field(default_factory=dict)
    score: dict | None = None

    def __post_init__(self):
        self.steps = self.raw["steps"]
        self.lint = self.raw["lint"]

    @property
    def ok(self) -> bool:
        return self.raw["failed_steps"] == 0

    @property
    def errors(self) -> dict:
        return {s["id"]: s["error"] for s in self.steps if not s["ok"]}

    @property
    def bodies(self) -> list:
        return self.raw.get("bodies", [])

    @property
    def volume(self) -> float:
        """The volume of all solid bodies together."""
        return sum(b["volume"] for b in self.bodies)

    @property
    def masses(self) -> list:
        """What it weighs part by part: its own bodies, and each part placed in it (by the
        step that placed it, `box/lid` within another), each with name, material, volume
        and mass in kilograms (None where its material is unknown: give the part a material
        of the folder's library, see `material`)."""
        return self.raw.get("masses", [])

    @property
    def mass_total(self) -> float | None:
        """What all of it weighs whose material is known, in kilograms (None if nothing's is);
        `unweighed` names what is left out."""
        return self.raw.get("mass")

    @property
    def unweighed(self) -> list:
        """The parts (and own bodies) `mass_total` leaves out: their material is not known."""
        return self.raw.get("unweighed", [])

    def mass(self, density: float, bodies=None, placed: str | None = None) -> float:
        """Volume × density for all bodies, or those named: density per cubic unit of the part
        (mm³ → g/mm³ is kg/m³ × 1e-6; in³ → lb/in³ is kg/m³ × 3.6127e-5). With `placed`, of
        the bodies of that placed part (see `placed_part`) instead of this part's own."""
        of = self.placed_part(placed)["bodies"] if placed else self.bodies
        return sum(b["volume"] for b in of if bodies is None or b["name"] in bodies) * density

    @property
    def placed(self) -> list:
        """The parts placed in this one (an assembly), and in them: each with name (the step that
        placed it; `frame/hinge` within another), file, variant, pose (`at`, `euler` degrees,
        `rotation` quaternion x y z w), material, volume, mass (kg, None where its material is
        unknown) and `bodies` — their faces and edges by name with positions where the part
        is placed, to pick mating faces by (`faces(..., placed=name)`)."""
        return self.raw.get("placed", [])

    def placed_part(self, name: str) -> dict:
        """One placed part of `placed`, by its name."""
        found = [p for p in self.placed if p["name"] == name]
        if not found:
            raise KeyError(f"no placed part {name!r}; there are {[p['name'] for p in self.placed]}")
        return found[0]

    def volumes_after(self, step: str) -> dict:
        """The bodies' volumes right after a step: to see what each step added or took."""
        return next(s for s in self.steps if s["id"] == step).get("volumes", {})

    def sketch(self, id: str) -> dict:
        """A sketch as the solver left it: solved, degrees_of_freedom, free_points, redundant and
        conflicting constraints (as you said them), `points` (name -> [u, v]) and `radii`;
        and `error` if its step failed."""
        step = next(s for s in self.steps if s["id"] == id)
        return dict(step.get("sketch", {}), **({} if step["ok"] else {"error": step["error"]}))

    @property
    def drawings(self) -> list:
        """The drawings: id, views, dimensions_shown, dimensions_missing, problems, and with
        render the paths pdf, svg, png."""
        return self.raw.get("drawings", [])

    def sheet(self, id: str | None = None, show: bool = True) -> str:
        """The inventory of a sheet (the first, or the drawing `id`) as compact text, printed
        unless `show=False`: its views, and per view every dimension and note with its text as
        printed, what it measures, whether its ends are on drawn geometry, where its text
        stands; centre lines, cutting lines, balloons, the parts list, the problems. The same is
        written to `<part>.views/sheet.txt` on every build."""
        found = [d for d in self.drawings if id is None or d["id"] == id]
        if not found:
            raise KeyError(f"no drawing {id!r}; there are {[d['id'] for d in self.drawings]}")
        text = "\n".join(sheet_text(d["inventory"]) for d in found if "inventory" in d)
        if show:
            print(text)
        return text

    @property
    def pictures(self) -> list:
        return self.raw.get("pictures", [])

    def _pick(self, kind: str, near=None, where=None, count: int | None = None, placed=None) -> list:
        of = self.placed_part(placed)["bodies"] if placed else self.bodies
        found = [x for b in of for x in b[kind] if where is None or where(x)]
        if near is not None:
            found.sort(key=lambda x: math.dist(x["middle"], near))
            found = found[: count or 1]
        return [x["name"] for x in found]

    def edges(self, near=None, where=None, count: int | None = None, placed: str | None = None) -> list:
        """Edge names: those `where(edge)` holds for (an edge has name, from, to, middle,
        length, straight), nearest to the place `near` first (then `count` of them, 1 if not said).
        With `placed`, those of that placed part (positions where it is placed; the names are
        its own: refer to them with `placed(name, topo(edge))`)."""
        return self._pick("edges", near, where, count, placed)

    def faces(self, near=None, where=None, count: int | None = None, placed: str | None = None) -> list:
        """Face names: as `edges`; a face has name, area, middle, normal, flat, and surface: what
        kind it is however it was made — "plane", "cylinder", "cone" or "other"."""
        return self._pick("faces", near, where, count, placed)

    def show(self) -> None:
        for s in self.steps:
            line = f"{'ok    ' if s['ok'] else 'FAILED'} {s['kind']:<14} {s['id']}"
            if "sketch" in s:
                k = s["sketch"]
                line += f"  dof {k['degrees_of_freedom']}, {k['dimensions']} dimensions"
            if "says" in s:
                line += f"  {s['says']}"
            if not s["ok"]:
                line += f"  {s['error']}"
            print(line)
        for b in self.bodies:
            print(f"body {b['name']}: volume {b['volume']:.6g}, centre {[round(c, 4) for c in b['centre']]}")
        for p in self.placed:
            print(f"placed {p['name']} ({p['file']}) at {[round(c, 4) for c in p['pose']['at']]}, "
                  f"turned {[round(c, 4) for c in p['pose']['euler']]}")
        for m in self.masses:
            weighs = "unknown" if m["mass"] is None else f"{m['mass'] * 1000:.6g} g"
            # (The density is the workspace library's for the material named, not a figure
            # the script states: said, so the two are not mistaken for each other.)
            density = m.get("density")
            said = f", {density:.6g} kg/m^3 from the library" if density else ""
            print(f"mass {m['name']} ({m['material']}{said}): {weighs}")
        if self.mass_total is not None and len(self.masses) > 1:
            left = f" (at least: {len(self.unweighed)} not weighed: {', '.join(self.unweighed)})" if self.unweighed else ""
            print(f"mass in all: {self.mass_total * 1000:.6g} g{left}")
        for d in self.drawings:
            print(f"drawing {d['id']}: {d['views']} views, {d['dimensions_shown']} dimensions shown, "
                  f"{d['dimensions_missing']} missing; {'; '.join(d['problems']) or 'no problems'}")
        for line in self.lint:
            print("lint:", line)
        if self.score is not None:
            print("scorecard:", self.score["line"])
        if self.view_images:
            where = os.path.dirname(next(iter(self.view_images.values())))
            print(f"view images in {where}: " + " ".join(os.path.basename(f) for f in self.view_images.values()))
        t = self.seconds
        if t:
            if self.reused:
                print(f"built: reused an earlier build of the same recipe ({t['total']:.2f} s)")
            else:
                print(f"built in {t['total']:.2f} s (steps {t.get('steps', 0):.2f}, drawings and lint "
                      f"{t.get('drawings and lint', 0):.2f}, pictures {t.get('pictures', 0):.2f})")
