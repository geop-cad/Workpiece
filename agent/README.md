# Building Workpiece parts from a script

The `workpiece` Python package (`python/workpiece/`, installed by `pip install workpiece`) is a
small layer over the `workpiece` command line (in a checkout: `cargo build --release -p workpiece-cli`).
`python -m workpiece` starts the desktop app, `workpiece` (or `python -m workpiece.cli`) is the command line, and
`python -m workpiece.tools` has the helpers (`help`, `sheet`, `crop`, `tile`). A script is the model: it says sketches with their
constraints and driving dimensions, features, and patterns; `Part.build(path)` writes the
`.wpc` file, solves and builds it, and returns a `Report`. `agent/examples/` has complete
scripts: `flange.py`, `pinned_plate.py` (assembly), `bracket.py` (a drawing built to a
declared original), `wood_box.py` (parts + assembly sheet with detail, section, parts list).

**Read the quick reference below; read a long section only when you need it** with
`python -m workpiece.tools help <topic>` (`help` alone lists the topics; `help drawing`,
`help centre-lines`, `help assemblies`, ...). Do not read this file whole on every pass.

## Quick reference

```python
import sys; sys.path.insert(0, "path/to/Workpiece/agent")   # the agent/ folder of this repository
from workpiece import (Part, original, view, projected, section_view, detail_view, note, balloon,
                   topo, datum, axis, placed, anchor, profile, material)
p = Part(unit="Inch" | "Millimetre", material="6061 Aluminium", title="NAME", file="x.wpc")
p.expect(original(views=[("front","Front"), ("iso","Pictorial")], numbers=[92, "R32", "Ø50", "(9)"],
                  notes=["(4) PLACES"], mass=(450.0, 0.3, "g"), units="mm"))   # once, from the original
```

**Sketch** (`s = p.sketch("XY"|"YZ"|"ZX"|datum(id)|topo(face), id=, show_in=, on_drawing=)`;
u,v = x,y on XY; y,z on YZ; z,x on ZX; coordinates are only a starting guess)
- geometry: `s.point(u,v,name)`, `s.line(a,b,name)`, `s.arc(centre,start,end,name,cw=)`,
  `s.circle(centre,r,name)`, `s.rect(prefix,u,v,w,h)`, `s.slot(...)`, `s.polygon(...)`,
  `s.dot(u,v)` (a hole's place), `s.construction(*curves)`, `s.project(name, edge)`,
  `s.rotation_axis(point, towards)` (a centre line without end); `s.origin`, `"u"`, `"v"`
- constraints: `coincident point_on horizontal vertical parallel perpendicular collinear tangent equal
  midpoint symmetric fix` — a human's geometric constraints, not computed coordinates
- driving dimensions (each takes `show_in= on_drawing= reference= at=(x,y) decimals= id=`):
  `distance_u(a,b,val)`, `distance_v`, `distance(a,b,val,far=)`, `point_line`, `radius(c,val)`,
  `diameter`, `angle(a,b,deg)`, `diameter_about(point,axis,val)`; `reference=True` is a driven `(60)`
- target 0 degrees of freedom: `report.sketch(id)["degrees_of_freedom"]`

**Features**: `p.extrude(s, d, combine="NewBody|Join|Cut|Intersect", symmetric=, through=, flip=, id=)`,
`p.cut(s, d | through=True)`, `p.revolve(s, about=None|"u"|"v"|axis_name|axis("Z")|topo(edge), angle=)` (no `about`: the sketch's one rotation axis, else u; "u"/"v" draw a rotation axis along the plane's axis into the sketch; a sketch's rotation axis `ax` is the datum axis `datum(f"{s.id}/{ax}")`), `p.hole(s_with_dots, dia, depth=,
kind="Counterbore"..)` (or placed on a face: `p.hole(None, dia, on=topo(face), at=(x, y) | off=[(ref, d), (ref, d)] | around=axis("Z"), radius=, angle=)`), `p.fillet(edges, r)`, `p.chamfer(edges, d)`, `p.shell(open_faces, t)`,
`p.feature_pattern([face], count, about=axis("Z") | along=axis("X"), spacing= | points=sketch | fill=topo(face), grid=)`, `p.feature_mirror([face], "Y")`,
`p.pattern(body, ...)`, `p.mirror(body, "Y", join=)`, `p.plane("Z", 20)`, `p.dimension(ref_a, ref_b, show_in=, reference=, kind=)`,
`p.on_sheet(step_or_dim, show_in=, at=, on_drawing=)`, any other step `p.op("Kind", id, **args)` (`workpiece kinds`).
`p.sweep(section, path, twist=, scale=)` (section a sketch or `topo(face)`; path a sketch or a curve body `topo(name)`),
`p.loft([s1, s2, ...], guides=[curve, ...], start="Free|Normal|Tangent|Curvature", end=, start_face=topo(f), end_face=, surface=)`
(raw: `sections=[{"of": ref, "end": "Free", "face": ref}]`, `rails=[{"of": ref, "role": "Path"|"Guide"}]`, `coil=axis(..)` for a spring).
Surfacing (`p.op("Surface", way=..., picked=[...])`, ways Copy Stitch Unstitch Delete Offset(`flip=`) Fill Trim Split Untrim Extend;
Delete takes a surface's faces, whole surfaces, or a solid's faces (closed over); Stitch turns all one way, a closed one a solid facing out);
`p.op("Boundary", sides=[{"edge": topo(e), "way": "Free|Tangent|Curvature", "face": topo(f)}])` fills a hole of any number of sides;
Fillet/Chamfer take a surface's inner edges too; `p.op("Mesh", ..., surface=True)` keeps a SubD a surface.
Curves in space (wire bodies to sweep along), one kind each: `p.op("CurveThrough", points=[topo(v), ...], closed=,
start={"way": "Free|Tangent|Curvature|InFace|Normal", "to": topo(edge)|axis(..)|topo(face)|datum(plane)}, end=)`,
`p.op("Helix", axis=axis("Z"), radius=, pitch=, turns=, grow=, start=deg, left=)`, `p.op("CurveIntersection", first=topo(f), second=topo(g))`,
`p.op("CurveProjection", onto=[topo(face)], curves=[profile(s) | topo(edge_or_curve)], along=axis(..) | None for a sketch's normal)`.
Emboss/engrave onto faces: `p.op("Emboss", faces=[topo(f)], sketch=profile(s), areas=[(u, v)], height=, engrave=,
normal_to_face=)` (the sketch's areas thrown along its normal; only the first face they meet takes them — the near side of a ring;
sides along the sketch's normal, or square to the face), or `outline="Curves", curves=[topo(curve_body) | topo(edge)]`: closed
curves already on the face (a projection; rings inside others are holes), raised square to it.
Pick faces/edges after a build: `rep.faces(where=lambda f: f["flat"] and f["normal"][2] > .99)`,
`rep.edges(near=(x,y,z), count=1)`. Every wrapper takes `id=`.

**Assembly**: parts `Part(file="a.wpc")`; `asm = Part(); asm.add_part(a, id="a", at=(x,y,z), euler=(..)
| mates=[mate(anchor(topo(face),"FaceCentre",instance="a"), anchor(...), "Fastened", flip_with=True)])`;
`asm.standard_part("SocketScrew", "M5", 12, mates=[mate(onto, seat(), "Revolute")])`;
`placed("a", topo(face))`; `rep.faces(placed="a", ...)`; `rep.masses`, `rep.mass_total` (kg).

**Drawing**: `p.drawing(views=[view("front","Front",(110,165), style="HiddenDashed"),
projected("top","front","Above",75), section_view("secA","A",(x,y),of="top"),
detail_view("det","top",(cx,cy),r,(x,y),scale=(5,1)), view("iso","Isometric",(x,y),scale=(1,2))],
scale=(1,2), format="A3", centre_lines=[...], notes=[note(None,(0,0),"(4) PLACES",dimension="corners")],
parts_list=True, balloons="auto", leading_zero=False, comma=False, decimals=, trim_zeros=True)`.
Sides: Front Back Left Right Top Bottom; Isometric IsometricFrontLeft IsometricBackRight IsometricBackLeft
Dimetric IsometricBelow{Front,Back}{Right,Left}. Front looks from +x (y right, z up): draw the front
elevation on YZ, the side on ZX, the plan on XY.

**Build and look** (cheap, in this order; never render or crop by hand):
1. `rep = p.build("x.wpc"); rep.show()` — steps, degrees of freedom, volumes, lint, scorecard, seconds.
   Identical rebuilds are reused; `render=False` (default) skips the sheet files.
2. `rep.sheet()` prints the sheet as text (every view, dimension text as printed, what it measures, whether
   its ends are on drawn geometry, notes, centre lines, cuts, balloons, parts list, problems);
   also `x.wpc.views/sheet.txt` on every build; the scorecard is in `show()` and `views/score.txt`.
3. `p.build(path, render=True)` — sheets as PDF/PNG plus one image per view `views/view-<id>.png`
   (about 1200 px, the view with its dimensions): open only the view that changed.
4. `python -m workpiece.tools crop sheet.pdf x y w h -o out.png` (mm of the sheet from its lower left) or on a
   PNG (pixels) for any other crop; `python -m workpiece.tools tile original.png --grid 3x2 -o dir` cuts an original into
   overlapping 2x tiles to read its small text once.
5. Mass: `rep.mass_total` (kg) with a material of the folder's library, else `rep.mass(density)`.

**Conventions that bite**: model the drawing's own numbers (no derived values: `76` is not typed when the
drawing says `92`, `4`, `9`); one feature + a pattern/mirror, never copies; sketch dimensions as the
drawing dimensions that view; blind hole floors are 118 deg (`point=180` flat); a hole without depth goes
through everything; names of faces/edges come from the report, never guessed; a dimension is shown only in
a view that sees it true (`show_in=`), hidden edges need `style="HiddenDashed"`; report tool faults, do
not work around them silently.

## How to work

Builds are quick (a second or two): build after every step or two. `workpiece report part.wpc
--times` prints the seconds each step, the drawing and the pictures took.

1. Write the script step by step and call `report = p.build("x.wpc")` often; `report.show()`
   prints every step (ok / FAILED with the error), each sketch's degrees of freedom, the bodies'
   volumes, and lint. `report.volumes_after(step_id)` says what a step added or took.
2. To pick faces and edges for fillets, shells, patterns, planes: build first, then
   `report.edges(...)` / `report.faces(...)` return names — filter with `where=` on an edge's
   `from`, `to`, `middle`, `length`, `straight`, or a face's `middle`, `normal`, `area`, `flat`,
   `surface` ("plane", "cylinder", "cone", "other": a revolved or bent plane is flat too);
   `near=(x, y, z)` sorts by distance and keeps `count` (1 if not said). A round rim is often two
   arcs: select by `where`, not by `near`, to get all of it.
3. `p.build(path, render=True)` also writes `path.views/{iso,front,top,right}.png` for a part
   without a drawing (`pictures=True` forces them; a part with a drawing gets its sheets
   instead: see "Checking against the original"): look at them and compare with the drawing.
4. Mass = `report.mass(density)` (volume × density; mm³ × g/mm³ is kg/m³ × 1e-6, in³ × lb/in³ is
   kg/m³ × 3.6127e-5), per body with `bodies=[names]`. `Part(unit="Inch")` for inch parts
   ("Millimetre" is the default).

## The drawing

### Views

`p.drawing(views=[...], scale=(1, 2), format="A3")` adds a sheet (third-angle by default; scale
is a pair of whole numbers). Views:

- `view(id, side, at)` — side "Front", "Back", "Left", "Right", "Top", "Bottom"; pictorial
  "Isometric" (from front, right, above), "IsometricFrontLeft", "IsometricBackRight",
  "IsometricBackLeft", "Dimetric" (ISO 5456-3), from below "IsometricBelowFrontRight",
  "IsometricBelowFrontLeft", "IsometricBelowBackRight", "IsometricBelowBackLeft"; or the id of
  a camera view step, or `view(id, direction=(x, y, z), up=(0, 0, 1), at=...)` (looking from that
  way; the camera view is made for it), for any other
  direction (`p.op("CameraView", "trimetric", looking="Current", ...)`, then
  `view("pict", "trimetric", at)`). `at` is the view's middle in mm from the sheet's lower left
  corner (A3 is 420 × 297).
- `projected(id, of, beside, distance)` — "Left", "Right", "Above", "Below" of another view, in
  line with it.
- `section_view(id, section, at, of)` — the model cut by a `Section` step; with `of` it stays in
  line with that view, like a projected one. `turn=90` turns it on the sheet (counter-clockwise,
  degrees) to stand as the original has it; a turned section stands where `at` says.
- `detail_view(id, of, centre, radius, at, scale)` — a circled part of a view, drawn larger:
  `centre` and `radius` in the model's lengths, from the middle of what `of` shows (across and
  up as that view shows them). A detail of an assembly's view shows the placed parts (a divider
  in its groove); lengths sent there (`show_in`) are drawn in the detail at its scale, between
  the edges it shows, with their true values. Its letter stands by the circle by itself
  (outside it, clear of lines and words, on a short leader where it must stand off);
  `letter_at=(x, y)` puts it there instead (mm on the sheet from the circle's middle).
  A length with only one end in the detail is drawn as a half dimension (ISO 129-1): one
  extension line, one arrowhead, its line running a little past the circle, the whole value; so
  is a length across the axis of a half section whose other end is not seen (a bore's Ø).
- `caption_at=(x, y)` on `view`, `projected`, `section_view`, `detail_view` puts the view's
  caption ("B–B", "A (5:1)", a scale of its own) there: its middle, mm on the sheet from the
  view's middle — it moves with the view. Else it stands over the view (a scale, under it).
- `style="HiddenDashed"` on a view (also `projected`, `detail_view`, `section_view`) shows hidden
  edges dashed — of an assembly's placed parts too. Use it where dimensions end on edges that
  are hidden (a floor, a wall's inside, a divider inside a box): a dimension whose end is on an
  edge the view hides and does not draw, or on nothing the view draws, is named in the
  drawing's problems.
### Centre lines and lettering

- **Centre lines** (ISO 128-2, long-dashed dotted narrow, a few mm past the outline, crossing
  at long dashes; continuous narrow on circles under 12 mm across):
  `p.drawing(..., centre_lines="all")` (default: of everything round — bores, bosses, turned
  bodies), `"holes"` (of holes only, told by their faces' geometry, whatever step made them),
  `"none"`, or a list, each in every view that can show it: a datum's id (`p.plane(...)`'s;
  "origin" a small cross, "origin/z" an axis; a plane shows only where it is seen edge on, as
  its trace), a hole's or round feature's step id or face name (its centre line; seen end on, a
  centre mark), `("pitch", "holes")` the pitch circle of three or more like holes about one
  middle; the list may also hold "holes" or "all": `centre_lines=["holes", "mid", ("pitch",
  "bolts")]`. One line said how it is drawn: a dict `{"of": "bore", "view": "top", "hide": True}`
  (a hole's centre line taken off), `"ends": (2, -1)` (mm on past the outline, or less),
  `"gaps": [(10, 18)]` (broken from 10 to 18 mm along it: over a gap between lugs),
  `"symmetry": True` (ISO 128-2 symmetry marks at its ends), `"label": "CL SYM"` (words on it, by
  itself at the end where they stand clear; `"label_at": "start" | "end" | mm`); `"of": "ZX"`
  (the origin's planes "XY", "YZ", "ZX") draws that plane's trace: a plane of symmetry. In the app a selected line shows how far its ends run on. A dimension that ends on a datum plane or axis (a plane's offset) draws that
  datum's trace. `line_width=0.7` sets the wide lines (narrow half of it; default 0.5 on A4–A2,
  0.7 on A1/A0); dashes follow ISO 128-2 (dash 12d, gap 3d, long dash 24d, dot 0.5d).
  `lettering=5` sets the height of dimensions' and notes' lettering (ISO 3098: 2.5, 3.5
  (default), 5, 7); views' names, section and detail letters and balloon numbers are a size up.
  With `centre_lines="holes"` the holes of placed parts (an assembly) get theirs too.
  A radius or diameter whose text stands outside the round is on a leader with the text level
  on a short shoulder (ISO 129-1); `aligned_leaders=True` turns it along the leader instead.
  `trim_zeros=True` writes no more decimals than a number needs, up to `decimals` (inch:
  `.5`, `1.125`, `2.75`; a dimension's own `decimals=` stays exact). `thru=True` makes a through hole's callout say `Ø6 THRU` (default for inch parts); words
  attached to a callout (`note(None, (0, 0), "4 PLACES", dimension="bore/callout")`) stand
  under its last line.
### Tangent edges, half sections, parts list

- `p.drawing(..., tangent_edges="Thin")` — smooth edges, where a fillet runs into the face it
  rounds, are drawn thin (default), `"Hidden"` not at all, `"Wide"` as other edges. Where one
  surface goes on across an edge (two halves of a tube) nothing is drawn.
- **Half sections** (ISO 128-3): `section_view(..., half="Left")` ("Right", "Above", "Below":
  the half of the view that is cut) — the other half is drawn uncut, the two divided by a
  centre line at the part's axis: the axis of what is round that the cut passes through
  (nearest the view's middle), else the Section's own origin; `half_only=True` leaves the other half out (a symmetric
  part's section from its axis outward). A section takes its own scale with `scale=(1, 2)`,
  written beside its name (`A–A (1:2)`). Each body or placed part it cuts is hatched its own way;
  hatching runs at 45° unless the cut's outline runs that way (then 135°, 30°, ...), closer in a
  thin wall.
- **Parts list and balloons** (an assembly, ISO 7573 / ISO 6433): `p.drawing(..., parts_list=True,
  balloons="auto")` — a table over the title block (item, quantity, name: the placed part's
  `Part(title="WOOD BOX")`, else its file's name; material; `parts_list=["item", "qty", "name",
  "material", "density"]` says the columns), a part placed several times one item; each
  item a balloon (its number in a circle, a leader ending in a dot on the part) in the first view
  that shows it, outside the view, clear of the rest. `balloons=[balloon("rod1", "front", (60,
  40)), "auto"]` puts one by hand (mm from the view's middle; `to=(x, y)` its leader's end) and
  the rest by itself. In the app: Format → Parts; drag a balloon to put it by hand.

### Which side is which

**Which side is which.** Workpiece's Front looks at the model from +x: the Front view shows y to
the right and z up — exactly a sketch on "YZ" (u right, v up). So draw the part's front
elevation on YZ, its side elevation on ZX (the Right view shows it with x running to the
left, z up), and its plan on XY (the Top view shows y to the right and x downward: a quarter
turn from how the sketch is drawn). If the original's front view shows the part's long side,
lay that side along y. Base the sheet on `view("front", "Front", …)` and project the others.

### Which dimensions appear

**Which dimensions appear.** Every driving dimension of every sketch, every extrude or
revolve's own distance or angle, and every `p.dimension(...)` step. To make the sheet read
like the original:

- dimension each sketch the way the drawing dimensions that view (same numbers, same
  references) — a derived number in a sketch shows up as a number the original does not have;
- `p.sketch(plane, show_in="top")` puts that sketch's dimensions in the view with that id;
- `p.sketch(plane, on_drawing=False)` keeps a helper or cutter sketch's dimensions off the sheet;
- `p.dimension(face_a, face_b, show_in="front")` adds a number no sketch carries (a depth, an
  offset) between two named faces or edges.

More for matching the original:

- **Steps say their own sizes.** A Fillet puts its radius on the sheet (`R8`, once), a
  Chamfer its distance(s), a Draft its angle, a Hole one callout (ISO 129-1) on one leader to
  a rim the view draws (the bore's, else a counterbore's) — `Ø6 ↧10` (depth only if blind; a
  tapped hole its thread `M6`), `4× ` before it for several holes (its places times what
  feature patterns and mirror images of the hole made after it repeat it), a counterbore
  `⌴Ø11 ↧6` or countersink `⌵Ø12 × 90°` under it; it is called `<hole id>/callout` and placed
  by one `at` (`p.on_sheet("bore", at=(30, 20))`). `p.on_sheet("bore/callout", count=6)` says
  the count outright (`count=False`: none); `p.drawing(..., places=True)` writes it as a line
  `(6) PLACES` after the callout instead of `6×` before it. A depth that follows from a face
  (`up_to=`, `head_up_to=`) is written, as a reference `↧(50)`, only with
  `p.drawing(..., reference_depths=True)` — an Extrude or
  Revolve its distance or angle (not when it ends "to next" or "to a face"), a Shell its wall
  thickness (once), a Rib its thickness, a Pattern its spacing (along a line)
  or angle (over less than a full turn), an offset plane (`p.plane`) its distance, a plane at an
  angle its angle, a frame its moves and turn. A fillet measured by width or distance says
  that. Steer or hide them per step: `p.on_sheet("round", show_in="top")`, `p.on_sheet("pocket", on_drawing=False)`;
  a placed part's own on an assembly's sheet: `asm.on_sheet(placed("box", "base_sk/width"),
  show_in="front")`;
  one of a step's several by its own name: `p.on_sheet("bore/callout", at=(30, 20))`,
  `p.on_sheet("plate/distance", show_in="top")` (a name the model does not say is an error).
- **Each dimension can go its own way**: `s.distance(a, b, 50, show_in="side")`,
  `s.radius(arc, 6, on_drawing=False)` — over what the sketch says for all of its.
- **Naming a sketch dimension**: `s.distance(a, b, 50, id="width")` calls it `<sketch>/width`
  (else `<sketch>/d<n>`, n a counter), to attach words to it (`note(..., dimension="base/width")`)
  or place it (`p.on_sheet("base/width", at=(0, -30))`).
- **A diameter on a turned profile**: `s.diameter_about(point, line, 50)` — twice the point's
  distance from the axis line, shown `Ø50` (use it where the original shows a diameter of a
  revolve's profile; `"u"` / `"v"`, a `rotation_axis` or any line is the axis). Draw the
  sketch's rotation axis with `ax = s.rotation_axis(point, towards)`: a construction line
  without end through `point`, towards another point (which it passes through too) or along a
  direction `(du, dv)` — three degrees of freedom, constrained like a line (`vertical(ax)`,
  `point_on`, `parallel`, `angle`). `p.revolve(s)` with no `about` turns the sketch about it. In
  the app a dimension from a point or line to it, its text placed across it, is a diameter; it
  stays one until its "Diameter" mark is cleared.
- **Driven (reference) dimensions** `(60)`: `s.distance(a, b, 60, reference=True)` (any sketch
  dimension) holds nothing — the sketch must be fully constrained without it — and shows what
  the rest makes it, in brackets; `p.dimension(f1, f2, reference=True)` likewise.
  Driven dimensions never count as constraints (degrees of freedom, redundant/conflicting).
  (In the app a dimension that would over-define the sketch is added driven, and the "Driven"
  mark in its panel switches it; to driving only where that does not over-define.)
- **To the side of a circle**: `s.distance(point_or_line, arc, 8)` measures to the near side of
  a circle or arc, `far=True` to its far side (a slot's whole length from a line to the far
  side of its end); `s.distance(circle_a, circle_b, 4)` is the gap between two circles, or
  the wall between one and another inside it.
- **Words on the sheet**: `p.drawing(..., notes=[note("top", (30, 20), "(2) PLACES")])` — their
  middle in mm from the view's middle. `note("top", (60, 50), "CENTER OF R75", to=(-120, 0, 0))`
  gives them a leader (ISO 128-22) to a point of the model (or an edge/face dict from the build
  report: its middle; or (x, y) in the view, model lengths from what it shows' middle): an
  arrowhead where that is on an outline, a dot within a face; `to=[a, b]` gives a leader to
  each (in the app: Add a leader, then drag its end).
  `note(None, (0, 0), "(4) PLACES", dimension="corners")` makes them belong to a dimension (a
  step's id, or a sketch dimension's name `sketch1/d4`): they stand with its text and go where
  it goes — under its value, turned as it is (level under a text on a leader's shoulder), or,
  where under it they would cross the dimension's own extension lines, arrows or leader, or
  run off the sheet's frame, on its line after the value (`.250 X 45° CHAMFER`); clear of other
  texts where one of those places is. Never across the dimension's own lines.
- **A detail view carries dimensions** when they are sent there: `show_in="<detail view id>"` —
  any that has at least one end in the detail (with the other out of it, a half dimension).
- **`p.dimension(face)`** of a round face is `Ø` if the face goes all the way around and `R` if
  it is part of a round; `kind="Radius"` / `"Diameter"` says it outright. Two round faces about
  one axis give how far one is off the other. An angle between two faces is the one between
  the walls through where they meet; `supplement=True` is what is left of a half turn.
- **Sections**: `p.op("Section", id="A", along="Plane", plane=axis("X"), offset=0.0)`, then
  `section_view("secA", "A", at, of="top")`: the cutting line with arrows shows in `top`;
  within 15 mm of that view's line the section is drawn in line with it, farther off it stands
  where `at` says. `p.op("Section", "A", along="Sketch", sketch=profile(s))` cuts along the
  sketch's lines as far as they go (what is left of them, between their ends): a line from a
  hub's middle to its rim cuts that half only, and its cutting line ends where the line does.

A step's own size (a chamfer's distance, a fillet's radius) goes in any view that sees one of
its edges true somewhere: a view from the side of a chamfered rim shows it where the rim turns
across the view.

### Angles

**Angles**: every angle shows its own value — a sketch's or a step's what it holds, a
`p.dimension(...)` what it measures (`supplement=True`: what is left of a half turn) — its arc
in the one of the two sectors with that value nearest its `at`, never the other angle. The arc
runs about the apex through `at`; where `at` lies outside that sector the arc runs on past the
nearer arm to the text. Where the arc between the arms is too short for the text (a small
angle), the text stands outside past an arm, on the arc run on, the arrows pointing in from
outside where there is no room for them either (ISO 129-1). Two lines that do not meet are
drawn on to where they would (extension lines from each edge to the arc, across the apex
where that lies off the part). An angle between two edges (`p.dimension(e1, e2,
kind="Angle")`) is at the point where they cross. In a detail, an angle shows if its apex is
in the detail or both its lines cross it.

### Radii and diameters

**Radii and diameters** (ISO 129-1): the arrowhead stands on the arc as drawn (of an arc, at
its point nearest the way to the text); the line runs through the arrow towards the centre;
outside the round a short leader from the text, pointing in; inside, from the centre (a
diameter of a whole circle across it, two arrows). `at` is where the text stands (in a detail
too, mm on the sheet from the detail's middle; of a round with several places alike — a bend
repeated — the one whose arc is nearest `at`). The leader is straight and radial, ending in a
level shoulder under the text: where the arc does not reach round to the side of the text (a
hollow round, the text off the part), it runs from the text straight through the centre to the
arc on the other side; where the text is off the radial line, the shoulder runs level from the
radius to under it. A round the view hides (behind something nearer, in a view without hidden
lines) gets its arrow on a short hidden-line arc where it is, and the build says it ends on a
hidden edge. A length can end at the middle of an arc the view draws (a bend's, a corner
round's): its end then counts as drawn and gets a centre mark. A diameter
of a circle no view draws (a bolt circle) draws that circle as a centre line. A radius seen
slantwise (a fillet on a drafted wall, within about 30° of square on) is shown as `TRUE R20`.
`p.dimension(face, datum("ZX"))`, `p.dimension(face, axis("Z"))`: to the origin's planes
("XY", "YZ", "ZX") and axes — an axis is taken as the line or the plane square to it, whichever
goes with the other.

### Where a dimension stands

**Where a dimension stands, and how it is written.**

- Every dimension call takes `at=(x, y)`: where its text stands, in mm on the sheet from the
  middle of its view (`s.distance(a, b, 10, show_in="top", at=(0, -40))`,
  `p.dimension(f1, f2, at=(55, 10))`, `p.on_sheet("fillet1", show_in="front", at=(30, 25))`).
  Without it the text is put clear of the view by itself and brought onto the sheet if it
  would fall off it. An angle's text near its apex, a radius off its arc: say `at`. An angle's
  arc is drawn about its apex through where its text stands (and on to it, if it stands past
  the arms).
- A dimension is only shown in a view that sees it true (not end on, not askew), and in a
  detail only if what it measures is inside the detail's circle. If the view you name cannot
  (or there is no view of that id), the build says so in the drawing's problems
  (`report.drawings[0]["problems"]`) and uses another; a dimension that no view shows is named
  there too. Name a view that sees it square, or dimension an edge that view does see square.
- An isometric view carries a length along one of the model's axes (a bracketed `(22)`) when
  it is sent there with `show_in`.
- `s.radius(...)`/`s.diameter(...)` with `show_in` puts that one dimension on the sheet even
  when its sketch is `on_drawing=False`.
- Numbers: `p.drawing(..., decimals=2, leading_zero=False, comma=False)`; an inch part writes
  ".750" (three decimals, a point, no leading zero) by itself, others "0,75"-style with no
  more decimals than needed. Any dimension takes its own `decimals=`.

### Round things in p.dimension

**Round things in `p.dimension`.** An arc or circle *edge* gives R (a whole circle Ø), also
a curved edge that is a circle within tolerance (the edge of a sweep along an arc); so does a
face that bends as a circle one way — a fillet around a rim, a ball, a swept bend
(`kind="Radius"`/`"Diameter"` to choose; `plain=True` writes the bare number, without R or Ø,
`plain=True, reference=True` "(3)"). A face that bends two ways (a swept bend of a wire) has
its own radius by default and the bend's with `bend="Middle"`, `"Inside"` or `"Outside"` (a Ø6
wire bent at R9: R3, R9, R6, R12). In a view that sees it from the side (a section of a
revolved part, a front view of a ring) a radius or diameter is drawn as a length across the
profile with its R/Ø; a ball's in any view. Between two round faces about one axis: the wall
or offset between them, also in a section.
Between two round things that are not about one axis, or a round thing and a flat face,
straight edge or datum (`datum(id)`, `axis("Z")`): their near sides by default, `far=True`
their far sides, `centres=True` their middles, `same_side=True` the far side of the one to the
side of the other that faces the same way (how far a rounded end stands out past another).
For things that do not lie square to each other say the direction: `along=axis("Z")` measures
only along it, from how far the one reaches that way to how far the other does (the bottom of
a bend below the top of a wire). A round face whose axis leans along that direction (the V
bends of a swept wire, a tilted rod) has no sides there: it is measured by its middle — a bend's
centre point, else the point of its axis nearest the face's middle — so
`p.dimension(bend_a, bend_b, centres=True, along=axis("X"))` gives the 100 between two bends
and never refuses; `centres=True` also takes a bend's centre (not its tube) without `along`.
A countersink's or counterbore's head: dimension its rim edge.
`report.show()` prints what each dimension step measured.

### Views of some bodies

**Views of some bodies, sections of the cut only.** `view(..., bodies=["plate"])` (also
`projected`, `section_view`) shows only those bodies: one body of several per view.
`section_view(..., cut_only=True)` draws only what the cut meets, hatched, with its outline.

### Where a dimension stands by itself

**Where a dimension stands by itself** (no `at`): clear of what stands on the sheet already —
in any view — of the views' names and letters, and of the other views; a length goes to the
other side of its view where the near side is taken by another view; two radii about one middle
get leaders of their own. A fillet's or chamfer's size is drawn at the corner nearest its `at`.
Extension lines start at the outline nearest their dimension line (with the sheet's gap), not
where on a face the length was taken.

### What the build reports

**What the build reports** (`report.drawings[0]["problems"]`): besides a dimension no view sees,
a dimension ending on an edge its view hides (draw the view `HiddenDashed`) or on nothing it
draws, an extension line or leader longer than 80 mm, texts written over each other, a text or a
view on the title block, a section that could not cut a body. Read them; a sheet a draughtsman
accepts has none.

`p.build(path, render=True)` writes `path.views/<drawing id>.png` (also .pdf, .svg) and one
image per view, `view-<id>.png`: look at it beside the original — same views in the same
arrangement, same dimensions with the same values, none extra, none missing. Compare first
as text: `report.sheet()` and the scorecard (next section).

## Several parts and assemblies

**Rule: a drawing whose parts list has several items is modelled as one standalone part per
item, plus an assembly that places them — not as several bodies of one part.** A box with two
dividers is `box.wpc` and `divider.wpc` (placed twice), and `box_assembly.wpc`. Each part
has its own material, mass and drawing; the assembly only places them.

```python
from workpiece import Part, anchor, placed, topo

plate = Part(material="Aluminium", file="plate.wpc")   # modelled as any part
...
pin = Part(material="Steel", file="pin.wpc")
...
asm = Part()                                              # the assembly: no bodies of its own
asm.add_part(plate, id="plate")                           # Fixed where it was modelled
asm.add_part(pin, id="pin1", at=(-20, 0, -5))             # the same part file, placed twice
asm.add_part(pin, id="pin2", at=(20, 0, -5), euler=(0, 0, 90))
report = asm.build("out/pinned_plate.wpc")              # builds plate.wpc, pin.wpc there first
report.show()
for m in report.masses:                                   # per placed part: name, file, material,
    print(m["name"], m["file"], m["mass"])                #   volume, mass (kg)
print(report.mass_total)                                  # all of it (kg)
```

- `add_part(part, ...)` takes a `Part(file=...)` of the script (built into the assembly's
  folder first, once however often it is placed; its report is `asm.reports[file]`) or the
  name of a .wpc file already in that folder. The step's `id` names the placed part.
- **Where it goes**: without mates it is fixed (`fixed=False`: free to drag) with `at=(x, y, z)`
  (where its origin goes) and `euler=(x, y, z)` (degrees about the fixed x, y, z axes, in turn);
  or held by `mates=[...]`, each `mate(onto, with_, constraint, flip_onto=, flip_with=,
  offset=, angle=, distance=)`: `onto=anchor(topo(face), "FaceCentre", instance="plate")` (a
  face of a part placed before), `with_=anchor(topo(face), "FaceCentre")` (one of the placed
  part's own), and the constraint between them — `"Fastened"` (default), `"Revolute"`
  (concentric and coincident), `"Slider"`, `"Cylindrical"`, `"Planar"`, `"Ball"`.
  `flip_with=True` turns the part's reference over, to have two faces face each other (a pin
  standing on a plate); `flip_onto=True` turns the other. Two mates hold what one leaves free
  (a revolute mate on an axis and a second one off it). Snaps: "Datum", "FaceCentre",
  "FaceAxis", "EdgeMiddle", "Vertex", "ArcCentre". The joints are named after the step (the
  second mate's `id/2`), for `Drive` and `Mechanism`.
- **Standard parts**: `standard_part(kind, size, length=None, mates=..., id=...)` — kinds
  "HexScrew", "SocketScrew", "HexNut", "Washer", "DowelPin", "Bearing". Each has a seat, the
  frame `seat()` on its axis where it naturally sits, z into it (a screw's ring under its head, a
  nut's or a washer's face, a pin's end, a bearing's side face): `mate(anchor(topo(top),
  "FaceCentre", instance="plate"), seat(), "Revolute", offset=(x, y, 0))` stands it on that face.
- **Off the reference's middle**: `offset=(x, y, z)` holds it that far off `onto`, in that
  reference's own frame — for a face, x and y in its plane (along the part's x and y for a top
  face) and z along its normal — after the joint lines the two up. For a face, x and y are the
  face's own axes (for a flat face parallel to the XY plane they follow the model's x and y), so
  the first number runs along that axis, not always along the part's long side. A divider fastened onto a
  floor 32 off the floor's middle is one step, no helper frame:
  `asm.add_part(divider, id="divider1", mates=[mate(anchor(topo(floor), "FaceCentre", instance="box"),
  anchor(topo(foot), "FaceCentre"), flip_with=True, offset=(32, 0, 0))])`.
  A step after the joints (a dimension, a drawing, a cut across parts) sees each part where its
  joints hold it, as the report does.
- **Picking faces of placed parts**: `report.placed` lists each placed part (name, file,
  pose, material, volume, mass, bodies with faces and edges); `report.faces(placed="pin1",
  near=(-20, 0, 25))` picks among that part's faces by where they are *in the assembly*; the
  names are the part's own. Refer to one from a step of the assembly with
  `placed("pin1", topo(name))` (e.g. `asm.dimension(placed("pin1", topo(a)), placed("plate",
  topo(b)))`).
- **Mass**: with the part's material known to the folder's `materials.json` (or the built-in
  library: Steel, Aluminium, Brass, Oak, ABS, …), `report.masses` / `report.mass_total` weigh
  it. A material the library does not have: `material(folder, "Cherry", 560)` adds it (kg/m³)
  to that folder's library before building; or compute it yourself with
  `report.mass(density, placed="pin1")`.
- A drawing of the assembly (`asm.drawing(...)`) shows the placed parts as a part's drawing
  shows its bodies; each part's own drawing goes in its own script part.
- `add_part(..., bodies=[...])` takes only some of a part's bodies: that is for splitting an
  existing part after the fact, not for parts you model — do not use it in these scripts.

See `examples/pinned_plate.py` (a plate and a pin placed twice, masses checked by hand).

## Checking against the original

The build can tell most of what a grader would find, as text, before any picture is opened.

**Declare the original once** (whoever first reads the drawing; later passes and the grader reuse it):

```python
p.expect(original(
    views=[("top", "Top"), ("front", "Front"), ("secA", "Section"), ("iso1", "Pictorial")],
    numbers=[92, 4, 9, 60, "R32", "Ø50", "(9)", "45°"],     # every dimension, one entry per occurrence
    notes=["(4) PLACES", "THRU", "CL SYM"],                   # words printed on it, one per occurrence
    mass=(450.0, 0.3, "g"), units="mm", decimal="."))        # (expected, tolerance, unit); units "in" drops leading zeros
```

View kinds: Front Back Left Right Top Bottom Pictorial Section Detail Auxiliary (ids are yours;
kinds are compared). Numbers are compared as printed (`R`, `Ø`, `°`, brackets for reference
dimensions); counts (`(4) PLACES`, `4×`) are notes, not numbers. For a multi-part script put
`expect` on the part whose sheet stands for the original's (the assembly).

**The scorecard** (`report.show()`, `report.score["line"]`, `<part>.wpc.views/score.txt`):
`views 6/6, numbers 21/25 (missing: 18, (9), R6, 60), extra: 5.4; notes 3/5 (missing: ...);
mass ok (450.016 g vs 450 ± 0.3); problems 0, ends off geometry 2`. Views by kind (missing /
extra views also appear as lint), numbers of the original not on the sheet and numbers on the
sheet not in the original, notes, mass in tolerance, number format (comma, leading zero), the
sheet's problems, and dimensions whose ends are not on drawn geometry. A producer works to a
clean scorecard and clean problems; a grader opens images only for what it cannot judge
(placement, legibility, right geometry).

**The sheet as text** — `report.sheet()` (also `<part>.wpc.views/sheet.txt`, every build):

```
view front  [Front] Front  1:1 HiddenDashed  at 110,165  size 127,69.85  box 38.5,113.07..217.3,221.43
  dim front/d54  ".875"  length  sketch front: distance p8 to p7 along U  ends ok  @-33.34,-49.18
  dim front/r50  "R.50"  radius  sketch front: radius of a2  ends -  @102.15,-43.25
  words "(2) PLACES" of front/r50  @122.07,-43.25
  line axis of cross "CL SYM"  -66.5,0 -> 66.5,0
```

One line per item: view id, kind (Front/Top/.../Pictorial/Section/Detail/Auxiliary), what it is of,
scale, style, place and box on the sheet (mm); per dimension its id, text exactly as printed, kind,
what it measures (a sketch dimension: the two points or curves by name; a `p.dimension`: the
references), `ends ok` or `ends OFF 0:3mm` (the end is that far from any drawn line or round: the
dimension points at nothing drawn), where its text stands (mm from the view's middle); words,
centre lines, cutting lines with their letters, balloons (item numbers), parts-list rows, and the
sheet's problems.

**Lint of the script** (in `report.lint`, printed by `show()`): a typed number that is not among the
declared numbers but is a sum, difference or half of them ("the value 79 is not on the drawing:
it looks like 92 − 4 − 9; model it from those numbers"); private members of the layer used
(`p._steps`, `rep._pick`); a script reading its own output files before building; and the view
kinds against the declaration.

**Pictures without hand work**
- `p.build(path, render=True)` writes `<path>.views/view-<id>.png` per view (the view's box plus its
  dimensions, about 1200 px on the long side) beside `drawing1.png` / `.pdf`; `report.view_images`
  lists them. Open the one view that changed, not the whole sheet.
- `python -m workpiece.tools crop sheet.pdf x y w h -o out.png` — a box of a sheet PDF in mm from its
  lower left corner (the `at` / `box` numbers of `sheet.txt`); on a PNG/JPG the box is in pixels from the
  upper left. Python: `from workpiece import crop; crop(path, (x, y, w, h), out)`. `-m 5` widens the box.
- `python -m workpiece.tools tile original.png --grid 3x2 -o tiles/` — overlapping tiles at 2x
  (`tile-r1c1.png` ...; `tiles.txt` says where each lies in the original's pixels): read the original's
  small text once, then reuse the tiles. Python: `tile(path, (3, 2), out_dir)`.

**Speed**: `report.seconds` / the last line of `show()` say where the time went (steps, drawings and
lint, pictures). A build of what an earlier build of the same path already built (same recipe, and
the parts it places) is not done again (`report.reused`); `render` defaults to False — ask for sheets
and view images only on the build you are going to read; the four iso/front/top/right pictures are
written only for a part without a drawing (`pictures=True` forces them). The `workpiece` command
line: `workpiece make recipe.json -o x.wpc --report r.json [--times] [--render dir [--no-pictures]]`;
the report's `drawings[i]["inventory"]` is the sheet as data.

## What a good model is (what is checked)

- **Every sketch fully constrained**: `report.sketch(id)["degrees_of_freedom"] == 0`, and the
  lint list has no sketch in it. Give the dimensions the drawing gives, as driving dimensions
  (`distance`, `distance_u`, `distance_v`, `radius`, `diameter`, `angle`, `point_line`), and
  geometric constraints (`horizontal`, `vertical`, `tangent`, `coincident`, `symmetric`, `equal`,
  …) — not computed coordinates. Coordinates are only the solver's starting guess: give a
  rough but plausible guess (right side, right order of magnitude).
- **No repetition that is a pattern or mirror**: one feature plus a `Pattern` (of features, or
  `of="Bodies"`), or a `Mirror` for a symmetric part, not the same feature several times. Lint
  flags three or more alike steps.
- **As few steps as do it**, with the ordinary operations a person would use (extrude, revolve,
  hole, fillet, chamfer, shell, rib, draft, pattern, mirror…).
- **The mass is right** within the drawing's tolerance.
- **The drawing matches**: a sheet with the original's views and dimensions (see above).

If an operation does not behave as a CAD user would expect, fails on valid input, or is
missing, do not work around it silently: report it (what you called, what happened, the error
text) — it will be fixed.

## Conventions

- **Sketch planes** `p.sketch("XY" | "YZ" | "ZX")` are the origin's planes; (u, v) of the sketch is
  (x, y) on XY, (y, z) on YZ, (z, x) on ZX — on ZX u is z and v is x (there is no "XZ"). `datum(id)` sketches on a datum plane a step made
  (`p.plane("Z", 20)` → the plane parallel to XY at z = 20); `topo(face)` on a flat face.
- **In a sketch** `s.origin` is the fixed point (0, 0); `"u"` and `"v"` are the plane's axes as
  construction lines (for `point_on`, `symmetric`, `point_line`, `angle`, `parallel`).
  Lines that share a point name are joined there; separate points need `coincident`.
- **Dimensions take their side from your starting guess.** `distance_u(a, b, 20)` says a and b
  are 20 apart along u; which of them is to the right is as you placed them. Values are
  positive. So place points on the right side of each other, roughly where they belong.
- **`angle(a, b, degrees)`** is from line a to line b, counter-clockwise, each line taken from
  its first point to its second. If a sketch will not solve or solves into a mirrored shape,
  an angle (or a guess on the wrong side) is the first suspect: swap the lines, or draw the line
  the other way round. `report.sketch(id)["points"]` shows where everything ended up.
- **Projecting the model into a sketch**: `s.project("rim", edge_name)` (names from a
  report) gives fixed construction geometry that follows the model — the curve `"rim"` and its
  points `"rim_a"`, `"rim_b"` (an arc: `rim_center`, `rim_start`, `rim_end`; a vertex: the point
  `"rim"`). Constrain against it (`point_on`, `coincident`, `tangent`, `distance`) so a line ends
  on a body's edge without a number being repeated.
- **A sketch on a face** (`topo(face)`) takes its axes from that face, not from the
  world: on a face that looks along −x, (u, v) is (y, −z). Check with one point and
  `report.sketch(id)["points"]` against the built body before drawing a whole profile; a
  datum plane (`p.plane("X", -30)`) keeps the world's directions.
- **Extrude up to a face** (`p.op("Extrude", sketch=profile(s), extent="ToFace",
  up_to=topo(face))`, or `up_to=datum(plane_id)`: it ends at that face's plane, on past the
  face's own rim, whichever way from the sketch that is. `extent="ToNext"` ends a cut where it
  first comes out (through all of the first wall, also where the profile crosses a gap).
- **A blind hole ends in a 118° drill point**; `point=180` makes its floor flat.
- **Holes** go at the *dots* of a sketch: `d = s.dot(u, v)`, dimension it, `p.hole(s, 6)`
  (`place="Sketch"`; a dot per hole). Or on a face without a sketch: `p.hole(None, 6,
  on=topo(face), off=[(topo(side), 10), (topo(edge), 15)])` (a distance from each of two
  planes, flat faces or straight edges — from a face of the body or an edge of the face drilled,
  inward), `around=axis("Z"), radius=20, angle=30` (from an axis, the angle from the face's
  x), or `at=(x, y)` (a point of the face's plane, in its own x and y). With
  no depth a hole goes through *everything* that way, bodies beyond included: give `depth=` or
  `target=topo(body)` when it should stop. A depth that is dimensioned elsewhere on the drawing
  is no number: `p.hole(s, 6, up_to=datum(plane_id), offset=0)` (or `topo(face)`) ends the bore at
  that face's or plane's plane (`offset` further in, negative short of it), and a counterbore's
  floor likewise, `kind="Counterbore", head=11, head_up_to=datum(plane_id)`; the callout then
  leaves that depth out (it is dimensioned elsewhere), or with `reference_depths=True` on the
  drawing shows it in brackets, as a reference. `next=True` ends it where it first comes out. Cuts and joins take `target=topo(body)` too when
  several bodies overlap.
- **Every wrapper takes `id=`** to name its step (`p.fillet(edges, 2, id="rim")`); names of
  faces and edges start with the id of the step that made them and change when an earlier
  sketch is redrawn or a pattern copies them (`pattern1/1/…`): list them again after
  such a change.
- **Helpers**: `rect`, `polyline`, `slot` (obround), `polygon` (dimensioned across flats). A
  `rect` is already horizontal/vertical: adding `symmetric` on top says some things twice —
  lint lists such redundant constraints; drop one of the two.
- **Patterns**: `p.feature_pattern([face_of_the_feature], count, about=axis("Z"), angle=360)`.
  A full turn is shared out evenly; over less than a full turn the first and last copy sit at
  the ends (3 over 180° are 90° apart). `along=axis("X"), spacing=…` for a row.
  `points=sketch` puts a copy at each dot of a sketch (moved there from where the feature
  is; none at a dot where it is already). `fill=topo(flat_face), grid="Square"|"Rectangular"|
  "Staggered"|"Hexagonal"|"Rings"|"Polygon", spacing=, across= (rows or rings apart), sides=,
  turned= (degrees), margin=` fills the face from where the feature is, leaving out what would
  cross its edges or the margin. `p.pattern(body_name or [names], …)` repeats whole bodies
  (the same ways; `combine="NewBody"|"Join"|"Cut"`, `target=` to cut). One step, `Pattern`,
  does both: `of="Features"` with `features=[faces]`, or `of="Bodies"` with `bodies=[...]`.
- **Mirrored features**: `p.feature_mirror([face_of_the_feature, …], "Y")` (a datum plane
  `datum(id)` or a flat face `topo(face)` also do) mirrors what the steps of those faces
  added or cut (a pocket with its fillet: one face of each step) in one step; what they make is
  named `{id}/…`. `p.mirror(body_or_names, "Y", join=False)` mirrors whole bodies (one step,
  `Mirror`, `of="Features"|"Bodies"`).
- **Planes from a face** (`p.plane(topo(face), d)`) have the face's own axes, the same as a
  sketch on the face: normal out of the body, origin = the world origin projected onto the plane,
  and "up" up: v is world +z projected onto the plane (normalised), u = v × normal, so the sketch
  reads the right way round seen from outside (a face looking along −x has u = −y, v = z). A top
  face (normal +z) has u = x, v = y; a bottom face (normal −z, seen from below) u = x, v = −y.
- **Cuts** turn towards the body by themselves; `through=True, symmetric=True` cuts through
  everything both ways.
- **Arcs** run counter-clockwise from start to end about their centre (`cw=True` for the other
  way). An arc's radius is free until dimensioned or its ends are otherwise fixed.
- **Extrude** goes along the plane's normal (`flip=True` the other way), `symmetric=True` both
  ways, `through=True` through everything; `combine` is "NewBody", "Join", "Cut", "Intersect".
- **Which areas of a sketch** an extrude (revolve, emboss) takes: `areas=[(u, v), …]`, a point
  inside each area in the sketch's (u, v) — `p.extrude(s, 5, areas=[(10, 5)])`, or
  `p.op("Extrude", …, areas=[(10, 5)])`. The point is looked up in the sketch as built, so it
  keeps working when curves or points are added to the sketch; put it well inside the area,
  not on a curve. Left out, the whole sketch is taken (areas inside others are holes). Areas
  picked in the app are saved by their boundary (`[{"entity": "e7", "left": true, "at": 0.5}]`,
  entity ids that shift when the sketch is redrawn): in a script, use points.
- **Every other step**: `p.op("Kind", **arguments)`; run `workpiece kinds` for all kinds with their
  arguments as by default — only say what differs. References are `profile(sketch)`,
  `topo(name)`, `datum(id)`, `axis("Z")`.
- **Names** of faces and edges come from the steps that made them and are stable across
  rebuilds as long as those steps do not change; get them from the report, do not guess them.
