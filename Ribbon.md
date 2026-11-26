# Ribbon

What each tab of the ribbon holds. ✓ marks what is implemented. Anything noted as "not implemented" is not there.
A feature with several ways of working is one button with a mode in its dialog (a fillet's
radius / width / distance, a pattern along a line / around an axis / along a path), not one
button a way.

## Sketch
(shown while a sketch is edited)

### Sketch
- Plane ✓
- Status (degrees of freedom) ✓
- Grid ✓
- Finish sketch ✓ (the Done button, or Enter)

### Draw
- Select ✓
- Line (chains into tangent arcs) ✓
- Rectangle (corner, centre) ✓
- Circle (centre, 2-point, 3-point) ✓
- Arc (3-point, centre) ✓
- Slot ✓
- Polygon ✓
- Ellipse ✓ (an exact closed curve by its hull; no ellipse constraints)
- Spline (control points, through points) ✓
- Point ✓
- Text ✓ (the letters' outlines as lines and curves, written from a point at a height; one font; held where written, not text that can be edited afterwards)

### Constrain
- Coincident / Concentric ✓
- Horizontal, Vertical ✓
- Parallel, Perpendicular ✓
- Collinear ✓
- Tangent ✓
- Curvature (G2) ✓ (a plain spline running on from the line, arc or spline it ends on)
- Equal ✓
- Midpoint ✓
- Symmetric ✓
- Fix ✓

### Dimension
- Dimension (length, distance, angle, radius, diameter; tolerances) ✓

### Modify
- Trim ✓
- Extend ✓ (a line on to a line or circle)
- Split ✓ (a line or an arc at a point)
- Sketch fillet, Sketch chamfer ✓
- Offset ✓
- Mirror ✓
- Move (drag) / Copy / Rotate / Scale ✓
- Linear pattern, Circular pattern ✓ (free copies: not tied to their original)
- Construction ✓
- Delete ✓

### Reference
- Project (edges, faces, vertices) ✓
- Intersect ✓ (the solid bodies cut by the sketch plane, drawn once as curves of the sketch's own)
- Convert a projection into free geometry ✓

## Solid

### Construct
- Sketch ✓
- Plane ✓ (offset, midplane, at an angle, through 3 points, through an axis and a point, normal to an axis, parallel through a point, tangent to a face at a point)
- Axis ✓
- Point ✓ (a frame at a picked point: Frame's "At point")
- Datum (frame) ✓
- Curve (project onto a face) ✓ (a curve body thrown onto a face along an axis)
- Curve (intersection of two faces) ✓ (where two picked faces cross, as far as both reach)
- Curve (helix / spiral) ✓ (a helix about an axis, or a spiral whose radius grows each turn — flat, without a pitch)
- Curve (3D spline through points) ✓ (a smooth cubic spline through picked vertices, frames and places on faces; each end free, tangent (G1) or curvature (G2) continuous to an edge or axis, lying in or normal to a face or plane — a held end flipped by a button, its tangent length (fullness, 1 natural) set by a number or by dragging the arrow at the end (through the end: flipped), lying in a face turned within it by a knob in its tangent plane; Sweep can follow any curve body)

### Create
- Extrude ✓
- Revolve ✓
- SweptLoft ✓ (sections as a list — sketches, faces' outlines; curves as a list, each a path or a guide: sweep one section along a path, mitered at its corners, with twist and scale; coil around an axis: springs; loft through sections, following guide rails, leaving its first and arriving at its last section free, normal to it, or tangent (G1) / curvature (G2) continuous to a face)
- Thicken Surface ✓ (faces or a whole surface, to one side or both; rounded or sharp corners)
- Solidify Surface Patch ✓ (no step of its own: a Boundary that closes the last hole of a surface makes it a solid; Offset by 0 of surfaces that close all around copies them into a solid)
- Primitives: none on the ribbon (box, cylinder, sphere, cone and torus steps of older parts still build)
- Import (STEP ✓; IGES ✓ — trimmed surfaces (144) and the faces of IGES's own solids (186 / 514 / 510 / 508 / 504 / 502), on NURBS surfaces (128), planes (108, 190), cylinders, cones, balls and tori (192 to 198), surfaces of revolution (120), tabulated cylinders (122) and ruled surfaces between like curves (118), bounded by 126 / 110 / 100 / 102 curves, sewn into solids where they close; offset surfaces are told of and left out; STL ✓ and 3MF ✓ — a closed mesh as a solid with flat faces, up to 20 000 triangles)

### Modify
- Fillet (constant, variable; rounded / mitered corners) ✓
- Chamfer ✓
- Shell ✓ (inward; open at picked faces, or closed)
- Draft ✓ (about a neutral plane; one way or both)
- Hole ✓ (simple, counterbore, countersink, tapped; for a screw size of the standards — M1 to M64: the clearance hole fine, medium or coarse (ISO 273), the counterbore for a socket head cap screw (ISO 4762, DIN 974-1) or a hexagon head (DIN 974-2), the countersink for a socket (ISO 10642, DIN 74) or slotted/cross recessed (ISO 2009, ISO 15065) countersunk screw, the tap drill for the coarse or a fine thread (ISO 261/262) — or to sizes of its own, a plain bore with its ISO 286 class (H7, F8, …); a tapped hole's thread cosmetic by default, or modelled; a deburring chamfer where it goes in, where it comes out, or both; at the points of a sketch, on a face a distance from each of two planes, or on a face a distance from an axis at an angle around it; to a depth or through)
- Thread ✓ (on a rod or in a hole, right- or left-handed; cosmetic by default — the face stays a cylinder, the thread shows as a helix on the model and is drawn as ISO 6410 has it: narrow lines along the roots and a wide one across its end from the side, three quarters of a narrow circle along its axis, dashed where hidden, the hatching of a section running to the crests — or modelled as a V groove; the ISO metric coarse thread for the face's diameter, or a pitch and depth of its own. Not implemented: cosmetic threads of placed parts in an assembly's drawing, a thread callout (M6) as a dimension, a thread shorter than its face)
- Rib ✓ (in the plane of its sketch, to the body)
- Emboss / Engrave ✓ (the areas of a sketch onto faces, flat or curved; text once sketches have it)

- Split ✓ (a body by a plane, a flat face, or faces of any shape that go all the way through it — a surface or some of its faces; not implemented: a face by a curve)
- Delete Body ✓ (solids, surfaces, meshes)
- Boolean (join, cut, intersect) ✓
- Transform (Move, Copy, Rotate, Scale, Mirror) ✓

- Linear Pattern ✓ (of bodies: as bodies, joined, or cut out of another)
- Circular Pattern ✓
- Path Pattern ✓ (along the curves of a sketch; turned with it or upright)
- Sketch Pattern ✓ (at the points of a sketch)
- Feature Pattern ✓ (the count a whole number; what steps did to a body — holes, bosses, pockets, fillets, several at once — done again along a line, around an axis, along a path or at points; the features are told by a face of each)

### Direct Edit
- Move / Offset Face ✓ (press-pull: off themselves or along an axis; neighbours that lie along the move run on, others are left with a step)
- Delete Face ✓ (Delete on the Surfacing tab: a hole, pocket or boss whose rims each lie in one face; not implemented: rounds, or what runs into a face's edge)
- Replace Face ✓ (by a plane: a way of Move Face; not implemented: by another surface)

### Inspect
- Measure ✓ (select one thing or two: its length, area or place, their distance and angle, read in Contents)
- Mass Properties ✓ (a step on the Properties tab: mass, volume, centre of gravity, the inertia tensor and principal moments of bodies taken together, each body by its own material; shown in the view as the ellipsoid that weighs and turns alike)
- Section Analysis ✓ (the bar over the view: the model drawn cut open as a Section step of the part cuts it, chosen from a list; the step's offset is dragged by an arrow while it is edited, the model shown cut)
- Interference ✓ (select two bodies: what they have in common, or their clearance)
- Curvature / Zebra ✓ (Contents → Display → Analysis: zebra stripes mirrored in the faces, or colours by how tightly they bend; and a selected face's tightest bend radius as a reading)
- Draft Analysis ✓ (Contents → Display → Analysis → Draft: faces coloured by their draft from a pull along z; and a selected face's draft, least and most, as a reading; not implemented: other pull directions)

## Surface

### Construct
(same as Solid; Curve through points is a large button of its own here as well)

### Create
- Extrude ✓ (a sketch, or any edges and curves of the model — in space too, several at once, connected ones giving connected faces — along a picked direction: an axis, a straight edge, a plane's or flat face's normal; without one, the normal of the plane the curves lie in. As a solid only curves closed in a plane)
- Revolve ✓ (a sketch, or edges and curves as for Extrude, about the picked axis)
- SweptLoft ✓ (as the solid's, without its flat ends; also through edges and curves left open — a loft between two curves is a ruled surface; a held end can be flipped, made fuller (tangent length) and, running on from a face, turned within it: numbers, and a knob in the face's tangent plane)
- BoundarySurface ✓ (over a hole of any number of sides — edges or curves end to end, one closed edge too — each side free, tangent (G1) or curvature (G2) to the face beside it, a held side flipped, made fuller and turned within the face's tangent plane by numbers or a knob at its middle; all free and in one plane: a flat face; four free sides: the Coons patch through them exactly; others fitted, within a stated tolerance of the sides. Sewn to the surfaces whose free edges it runs along; closed all around by that, a solid — this is also Fill hole)
- SubD ✓ (a subdivision surface from a patch: a surface body, not a solid, even when it closes)
- FromSolid (copy faces) ✓ (Offset by a distance of 0)

- OffsetSurface ✓ (each face its offset, sewn where they still meet; they do not run on to each other; to the side the faces' normals point, or flipped; by 0, a copy of the faces — closed all around, a solid)

### Modify
- Split ✓ (as Emboss takes its outlines: Sketch — the areas of a sketch thrown onto the faces along its normal, only the first face they meet — or Curves on face — closed curves already on them (a projection, edges); the faces of a surface or a solid are split in place. Or Plane: a surface in two. Not implemented: by another surface)
- Trim ✓ (no step of its own: Split, then Delete what is not wanted)
- Extend ✓ (past its free edges, by a distance; or To the whole surface — Untrim: every face back to all of its surface)
- Fillet, Chamfer ✓ (the solid's steps: edges of a surface between two of its faces too; not implemented: between surfaces that do not meet in an edge)
- Delete ✓ (faces of a surface or whole surfaces; faces of a solid — a hole, a pocket, a boss — taken off and closed over: Delete Face is this button)

- Transform (Move, Copy, Rotate, Scale, Mirror) ✓ (the solid tab's Transform takes surfaces too)

### Inspect
- Measure ✓ (as for solids: a surface's area, a face's area, an edge's length, distances and angles)
- Curvature / Zebra ✓ (Contents → Display → Analysis: zebra stripes mirrored in the faces, or colours by how tightly they bend; and a selected face's tightest bend radius as a reading)
- Open Edges ✓ (select a surface: how many of its edges are open, and how long they are together; not implemented: shown on the model)

## Assembly

### Part
- Insert part (a part file or a STEP file) ✓ (its dialog: the part, its variant, which of its bodies come along — a table, a tick and a "solo" button a row, the body under the pointer shown in the view, "Show all" bringing them all again — and how it is held)
- New part (made in place) ✓
- Standard part ✓ (one catalogue searched by typing — hexagon head screws ISO 4017 and socket head cap screws ISO 4762 in their lengths, nuts and washers M3–M16, dowel pins ISO 8734, eight ball bearings — with filters for the kind of part and the size; each kind has a seat — a screw's ring under its head, a nut's or a washer's face, a pin's end, a bearing's side face — that its first mate starts at, so one pick of a face or a hole's rim stands it there, revolute (concentric, coincident); more mates as for any placed part; threads, sockets and bearing balls drawn plain; in millimetres)
- Replace part ✓ (edit the step that places it and choose another file)
- Pattern part ✓ (each a step — a move, a turn — from the one before; fastened to the first)
- Mirror part ✓ (the part's bodies mirrored in a plane; fastened to the part; not implemented: sub-assemblies)

### Position
- Connector (a frame on a part to join by) ✓
- Joint (rigid, revolute, slider, cylindrical, planar, ball) ✓ — a placed part (Add part, Standard part) is held "By its mates" or "Fixed". By its mates: a table of mates, rows added and removed, each a reference on what is there (Onto) and one on the part (With), each with a button that turns it over, and the constraint between them from a drop-down with icons of the joints that fit the two (two axes or rims turn in each other by default, anything else is fastened); how far off and what it lets go under the row, opened; two mates hold what one leaves free; no pose of its own: a part not yet held is dragged where it goes. Fixed: its pose typed or moved by a gizmo; its mates are kept, hidden, and hold nothing till it is held by them again
- Ground (fix a part where it is) ✓ (a part's "Fixed" way of moving)
- Move (drag within what the joints allow) ✓ (the left button drags a placed part, while nothing is edited and while a step is — a part being placed too: the innermost part that its mates leave free to move, in the assembly that places it, which keeps where it went — in a placed sub-assembly too; one they hold, the assembly around it; Shift+drag moves the assembly around it instead (a screw that only turns in its hole looks as if nothing moved, and the status line says so). A fixed part stays, and the status line says why nothing moves: fixed, held by its mates, or driven)
- Mechanism (gear, rack and pinion, screw: one joint driving another) ✓ (its joints are pointed at in the view: while a joint field picks, the joints it takes are drawn where they are, highlighted under the pointer and named beside it)
- Joint limits ✓ and drives ✓ (Drive holds a joint at an angle or a distance; with a parameter there, the variants run it through its range; and Contents → Display → Motion runs a joint that has limits from one to the other by a slider, the parts drawn where that leaves them)

### Assembly Features
(what is done to the parts once they are put together)
- Cut / Hole through several parts ✓ (Cut parts: bodies of the assembly cut out of every placed part they run into; the parts' own files stay as they are)
- Weld bead between parts ✓ (Weld: a round bead along picked edges of placed parts, less what the parts take up — a quarter round in a corner; a body of the assembly)

### Inspect
- Measure ✓ (select things of placed parts as of the part itself: one is measured in its part's own coordinates, two across the parts as they are placed)
- Interference / Clearance (between parts) (which placed parts run into each other, and by how much, and the least clearance between parts whose boxes are apart (from points along their edges), in Contents ✓)
- Section Analysis ✓ (as for parts: cut through every placed part)
- Mass Properties (of the assembly) ✓ (volume and centre of gravity of all that is placed, in Contents; not implemented: mass, inertia)
- Exploded View ✓ (Contents → Display: the placed parts drawn apart from their middle; view only, no steps or trails)
- Bill of Materials ✓ (the parts placed, counted, in Contents)

## Manage
(the same in every tab: beside the ribbon, or a tab of its own)
- Parameters ✓
- Variants ✓
- Material ✓ (what a part is made of: chosen in Contents or in the Materials panel of the Properties tab, Ctrl+6; its bodies show as it and weigh by its density)
- Materials ✓ (the library every part chooses from: `materials.json` beside the parts, written as it is changed in the Materials panel; each material has a density, a colour, a texture drawn over it, and how it takes light — roughness, metal, clear. Textures are patterns worked out where the face is, not pictures)
- Appearance ✓ (a step: bodies or single faces get a material or a colour of their own, over what the part is made of; it stands in the timeline like any step)
- Render ✓ (a step that is a picture of the model as it is there: traced with light, shadows, reflections and glass, shown as it adds up while the view is turned as ever; studio, outdoor or dark-room light, a floor that catches the shadow, a backdrop colour; saved as PNG. Not implemented: pictures as textures, lights of one's own, depth of field)
- Units ✓ (a part's unit of length — mm, cm, m, in — chosen in Contents; STEP and 3MF exports say it; not implemented: converting numbers, mixed units in one field)
- Show / Hide ✓, Isolate ✓ (double-click a body in Contents)
- Pick ✓ (pointing at the model, in every field and for selecting: what is in front comes first — an edge or a vertex behind a face never before that face; of what lies at about the same depth (within two pick radii), a vertex before an edge before a face, so an edge of the face under the pointer still comes first; what a section cuts away and hidden bodies are not picked; the right button goes on to the next under the pointer, drawn over everything so one behind others shows, its name beside the pointer)
- Export (STEP ✓, IGES ✓, STL ✓ and 3MF ✓ from File → Export…; a drawing as DXF ✓ or PDF ✓ — see Drawing below)

## Further tabs and workbenches
Features that sit outside the tabs above, as implemented (✓) and not implemented:
- Annotate (Ctrl+5) ✓: what is said of the model on it — Datum (a face named A, B, C: ISO 5459), Dimension (as picked: of a round face its diameter, of an edge its length, of two things their distance or angle; with its tolerance: ISO 129-1) and Tolerance (a geometric tolerance of a face, of its form or from datums: ISO 1101). Each is made as a sketch's dimension is: pick what it is of, then click beside the model where it stands — the tool goes on to the next. A sketch's dimensions are said of the model too, and so are the sizes steps hold it to (an extrude's distance, a revolve's angle, a hole's diameter and depth, the distance of a thicken, an offset, a moved face, a shell's wall, an emboss's height). Every measure of a step takes a tolerance, typed after the number (`10 ±0.1`, `10 +0.2/-0.1`, `10 +0.2/-0.1 limits` for the two limits of size, `10 H7`) or chosen from the ± beside its field. With the Annotate tab open all of them show on the model as a drawing has them (ISO 16792): dimensions with extension lines, dimension line, arrowheads and the value over the line, in the plane they lie in (a sketch's, in the sketch); datum boxes with their triangle on the face and tolerance frames with the characteristic's symbol, facing who looks. Each is dragged by its text.
- Camera view ✓ (Annotate → Camera view: a step that is a named way of looking at the model — as the view is turned while it is edited, from a side, or square onto a plane, a frame's plane or a flat face, turned by an angle; the bar over the view sends the camera to it, and drawings draw the model from it)
- Section ✓ (Annotate → Section: a step that says where the model is cut open — along a plane with an offset, or along the lines of a sketch, one or several end to end (an offset section), square to the sketch's plane. The view shows the model cut so, and the section views of drawings are of it: they follow the part)
- Properties ✓ (Properties → Properties: what the part says of itself — part number, revision, author, date, status — and, under Define properties, which properties the parts of the workspace have and what each is: text, whole number, number, date, yes/no or one of a list, with a default and whether it is required; kept in `properties.json` beside the parts. Title blocks read them)
- Sheet templates ✓ (Annotate → Templates: what new drawings start from — the sheet's size and projection, the views it starts with (sides, camera views of the part, projected views), and the title block cell by cell: where it is, its label, and whether it says text of its own or a property of the part or the sheet (Name, Material, Mass, Scale, Format, Sheet, Projection, or any property of the workspace); kept in `sheets.json` beside the parts; ISO 7200 blocks on A3 and A4 to start with)
- Drawing ✓ (Annotate → Drawing asks what the sheet starts with — nothing, or the views of a sheet template — and adds a step that is a drawing of the model as it is there — in the timeline like any step, edited like a sketch in a workbench of its own; the code is the `ops/drawing` plugin. A sheet A4 to A0 with frame, centring marks and grid (ISO 5457) and title block (ISO 7200); views of the model from any side, projected views in line with them in first or third angle, auxiliary views along any direction of a view, views as the part's camera views look at it, sections of the part's Section steps with hatching, their cutting line with arrows and letters in a view that looks along the cut, nothing drawn where an offset section steps aside (ISO 128-3) — a cutting line drawn corner by corner in a view makes the part a sketch of it on the plane that view looks onto and a Section step along it, and dragging its corners draws that sketch anew — details at a larger scale, whose middle and reach are dragged afterwards; beside the sheet the timeline stays as everywhere, and the dialog at the right says what is selected on the sheet: with nothing selected the sheet itself (size, how it lies, scale, projection, template), a view (what it shows — a side or one of the part's camera views, changed at any time — its section, lines, scale, letter), or the title block (a click on a cell selects it and goes to that cell's field): text of the sheet's own, or the part's properties; each view with hidden lines removed or dashed, as a wireframe, or shaded; centre lines of what is round. "Show missing" draws what is said of the model and is not on the sheet in blue as what it would be — in the selected view, or in every view that sees it true, stacked 10 and 7 mm off the outline — and a double click puts it on the sheet. No line runs through a dimension's value: lines that would are interrupted. Dimensions of the sheet's own as in a sketch: click two points, a line or a circle, then beside it. Draw puts the tools of the sketcher on the sheet (lines, circles, arcs, splines, lettering, constraints, dimensions), snapping to the views. Export as PDF, DXF (a layer per kind of line) or SVG, true to size. Not implemented: several sheets of one drawing numbered together, broken views and broken-out sections, sections turned into the plane (aligned sections), a parts list with balloons, surface texture and welding symbols, thread and hole callouts, ordinate and chained dimensions. File → Export… as .dxf or .pdf without a drawing open still writes the three views and a section by itself)
- Sheet Metal (flange ✓ and bend ✓: Flange, on the Sheet metal tab (Ctrl+4), bends a flange up along a straight edge of a plate; unfold ✓ and flat pattern ✓: Unfold lays the bent sheet out flat from a picked face as a new body, each bend as long as its neutral layer by the K-factor; a tab of its own ✓ with sketch, datums, extrude, flange, unfold, hole and fillet; not implemented: reliefs at corners, hems and jogs)
- Mesh / SubD ✓ (no tab of their own: SubD is on Modeling, Mesh on Surfacing): SubD and Mesh add a step that is a control mesh — points, and faces of any number of corners — edited in the scene in a workbench of its own; the code is the `ops/mesh` plugin. The body it makes is the mesh's flat faces (Mesh), or the subdivision surface it controls (SubD: Catmull–Clark) as bicubic NURBS faces that share their edge curves — a solid if the mesh is closed, a sheet if not — on which every other tool works. Where four quads meet at each corner of a face the face is one NURBS face and the very surface; a face with another corner is four faces of the mesh one step finer, and only the quarter at that corner comes close to the surface (Finer: more steps). Creased edges and the rim of an open mesh stay sharp. Lines — free edges, of no face — make curves the same way: each run of them a wire body `{step}/curves` whose edges (`{step}/curves/e0` …) later steps take as any curve (sweep path, loft rail …): smooth, the limit curve of its subdivision (a uniform cubic B-spline over its points, through its open ends, closed round a loop, sharp at a point made a corner by Crease); flat, the run itself.
  - Select: points, edges or faces, one choice of three pictures (1, 2, 3); click, Ctrl-click, a frame dragged open; double-click an edge for its loop, a face for all that hangs together; in the context menu (and by their keys): All (Ctrl+A), Invert (Ctrl+I), Linked (Ctrl+L), Loop (L), Ring (Shift+L), Grow (Ctrl+G), Shrink (Ctrl+Shift+G), and the select tool back (Esc).
  - Move: a gizmo in the middle of what is picked moves and turns it (along the world's axes or the normal), scales it all at once by its outer ring (the factor shows while it is dragged), what is picked can be dragged itself, an arrow along each axis on the gizmo's far side stretches it; the factors and moves can be typed (X, Y, Z, All); Falloff takes the mesh around along.
  - Snap (as Rhino's object snaps, with a toggle each and one for all, kept in the settings; Alt held moves freely): points (of the mesh, of other bodies, of sketches, datums' origins), middles of edges, centres (circles, arcs, flat faces, the mesh's faces), quadrant points of circles, crossings of edges as seen, the perpendicular from where a drag started, onto an edge, and the grid — not drawn, its spacing following the zoom as the sketcher's does; along a gizmo arrow the snap target is seen along the axis, and the grid gives whole steps of that axis. The target shows with the sketcher's dot and badge.
  - Add: Box, Plane, Cylinder, Cone, Sphere (of quads), Torus — 10 mm across (a box's edge, a plane's side, the diameter of the others) — with size and pieces; Draw face, corner by corner; Line, point by point (one click: a point by itself).
  - Edit: Extrude (faces; edges; lines: a face along each; points: a new point joined by a line, on along the line it ends — a line drawn by extruding again and again), Inset (with depth; together or each face), Bevel (edges, one segment), Loop cut (cuts and slide), Knife (from point to point on edges and corners), Subdivide, Connect, Bridge (two runs of edges, two faces), Fill.
  - SubD: Crease (edges stay sharp; points of lines become corners of their curve), Finer.
  - Remove: Delete, Dissolve (points, edges, faces: no hole left), Merge, Collapse, Weld by distance.
  - Shape: Relax, Flatten, Duplicate, Split off, Inflate, Thicken, Flip, Outwards, Triangles, Quads, Poke.
  - Surface: Smooth (the subdivision surface, or the flat faces), Mirror X/Y/Z (the mesh is cut to its upper half and mirrored; points in the plane stay in it), Apply mirror.
  - What was done last keeps its numbers in the ribbon (distance, width, cuts …), to be typed anew.
  - Not implemented: a mesh made of a body or read from a file into the step (OBJ, STL as a mesh to edit), bevels of several segments and of points, edge and point slide, spin, sculpting, edge weights between smooth and sharp, hiding parts of the mesh, tangent snaps, a grid fill, UVs.
- Mesh (import ✓ and convert to solid ✓: STL and 3MF read as solids with flat faces; repair ✓ as far as it is plain — doubled triangles dropped, all turned one way, holes closed from a corner; not implemented: editing a mesh, smoothing, self-intersections)
- Simulation, CAM (CAM: File → Export… as .nc ✓ — the part's outline halfway up it as G-code for a laser, plasma or water-jet cutter, holes first; nothing allowed for the width of the cut. Simulation: motion ✓ — a joint run through its range by a slider, see Assembly. Milling ✓, a first kind: File → Export… as .tap — the same outlines milled from the part's top to its bottom in passes, with cutter compensation, for a tool 6 mm across in passes 2 mm deep. Not implemented: pockets and faces, turning, a choice of tools and feeds, stress or flow simulation)
