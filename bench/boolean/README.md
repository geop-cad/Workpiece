# The boolean benchmark suite

The thorough tests of `lib/boolean` and of the STEP importer: 1917 cases that take about half a
minute on 32 threads (minutes on a laptop). They are not part of `cargo test`; run them when the
boolean code or the STEP importer changes. How the results compare with other projects' is in
[`docs/benchmarks`](../../docs/benchmarks/README.md).

## Method

Each case builds two solids (or one, or a STEP file), applies the operation, and is graded as in
brep-bench:

- **pass**: a valid, closed solid (written to a topology and validated there) with the expected
  volume, and area where it is known, to a relative 1e-6. Volume and area are measured from the
  surfaces and loops themselves (`shapecraft_boolean::measure`), not from a mesh.
- **refused**: the operation declined, with a reason.
- **wrong**: anything else. Refused ranks above wrong.
- **crashed**: a panic.

Cases without a closed form are checked by what holds of any boolean:
`|A ∪ B| = |A| + |B| − |A ∩ B|`, `|A − B| = |A| − |A ∩ B|`, `|B − A| = |B| − |A ∩ B|`.

The run exits with an error if a case that passes in `baseline.tsv` no longer does, or if a case is
wrong or crashes that was not before. The per-case results of the last run are in
`target/bench-results/boolean.tsv`.

## Running it

```bash
cargo build --release -p shapecraft-boolean-bench
target/release/boolean-bench fetch                # once: download the datasets (about 270 MB)
target/release/boolean-bench                      # run every case
target/release/boolean-bench sphere tangent       # some categories, or case names
target/release/boolean-bench --bless              # accept the results as the baseline
```

`cargo run --release -p shapecraft-boolean-bench -- <arguments>` does the same.

## Cases

| Category | What |
|---|---|
| `box`, `cylinder`, `sphere`, `turned` | Pairs with volumes in closed form: crossing, nested, apart, through and blind holes, cones and rings, turned every way |
| `coincident` | Faces in one plane, on one cylinder or sphere; whole solids the same |
| `tangent` | Solids that touch along a line or at a point |
| `nearmiss` | Faces brought within ¼ to 10⁴ tolerances of each other |
| `chain` | Many booleans on one body |
| `random` | Random pairs of boxes, cylinders and balls: the four booleans must agree with each other, and the common volume with a count of random points |
| `features` | Cases of our own (`src/features.rs`), each with an answer in closed form and graded to the same relative 1e-6: standard parts by their sizes in the standards, where faces cross, surfaces trimmed by a plane and rounded along an edge, a closed mesh read as a solid, a STEP file written in inches and read back, a curve through points and a helix's length. `features_occt.py` runs the same cases with Open CASCADE (cadquery-ocp) for comparison |
| `brep-*` | The cases of [brep-bench](https://github.com/bmander/brep-bench), built by `src/brepbench.rs` and graded by its rule (volume and area to a relative 1e-6): `brep-boolean`, `brep-tangent`, `brep-fillet`, `brep-chamfer`, `brep-geometry`, `brep-parts`, `brep-nurbs` (one case more than brep-bench's), `brep-spline`, `brep-singular`, `brep-primitives`, `brep-sweep`, `brep-loft`, `brep-swept`, `brep-feature`, `brep-edit`, `brep-offset`, `brep-shell`, `brep-thicken`, `brep-draft`. The expected values are in `src/brepbench_expected.rs`. `boolean-bench brep-run CASE OUT.stl` is a runner for brep-bench's own harness |
| `step-conformance` | The 27 parts of [step-conformance](https://github.com/dan335/step-conformance), each read and meshed at its four tolerances (108 cells), scored by `src/conformance.rs`, a port of its rule |
| `step-import` | 675 public STEP files: each has to import as solids that are valid and closed |
| `step` | The parts of the first sets, each halved, sliced, drilled and combined with itself, the results checked against each other |

`rounds_occt.py` rounds single edges of the NIST and FreeCAD parts with Open CASCADE for comparison with
`boolean-bench fillet-corpus`; it needs cadquery-ocp.

## Datasets

`datasets.tsv` pins every file by address and SHA-256; `fetch` puts them in `target/bench-data`. They
are not part of the repository.

| Set | Source | Terms |
|---|---|---|
| `nist` | [NIST MBE PMI Validation and Conformance Testing](https://www.nist.gov/el/systems-integration-division-73400/mbe-pmi-validation-and-conformance-testing/download-free-cad): the CTC, FTC and STC parts, exported from several CAD systems | No restrictions; NIST asks to be acknowledged |
| `kicad` | [KiCad 3D model library](https://gitlab.com/kicad/libraries/kicad-packages3D), a sample of packages, at a pinned commit | CC-BY-SA 4.0 with an exception |
| `freecad` | [FreeCAD library](https://github.com/FreeCAD/FreeCAD-library), a sample of parts, at a pinned commit | CC-BY 3.0 |
| `step-conformance` | The parts and truth files of [step-conformance](https://github.com/dan335/step-conformance) | CC0 |
| `kicad-more`, `freecad-more` | 605 more files of the same two libraries, spread over all their folders; imported only | as above |

## STEP conformance

step-conformance grades STEP importers: each part is read and meshed at 0.0125, 0.05, 0.1 and 0.5
mm, and a cell passes if the mesh has every face of the file, reaches no further than the file's
geometry, and is within four tolerances of the true surface, both ways, in under 60 seconds.
`src/conformance.rs` applies that rule with what is known here:

- every part has to import as valid, closed solids with as many faces as the file has, mesh into
  triangles that close up, and take under 60 seconds;
- a part of known shape (its `.truth.json`) has every vertex and triangle middle within four
  tolerances of the true surface, every one of some 200,000 points of the true surface within four
  tolerances of the mesh, and, stricter than the benchmark, the solid's own volume equal to the true
  one to a millionth;
- the freeform panel is held to the same distances both ways, against its true walls, floor and flat
  ends;
- the shop parts, whose truth in the benchmark is OpenCascade's own fine mesh, are held to the first
  rule and to the size the benchmark gives for them; ShapeCraft's own fine mesh stands in for the
  reference reader, so those 28 cells are a weaker check than the benchmark's.

The benchmark's own scorer takes a reader as a program: `boolean-bench step-read OUT TOLERANCE FILE`
writes what its README asks of one.

## Current results

2026-11-25, 32 threads, commit `450425c` with uncommitted changes: 1917 cases in 23.6 s, **1902
pass, 12 refused, 3 wrong, 0 crashed**. By category:

| Category | Cases | Pass | Median per case |
|---|---|---|---|
| `box`, `cylinder`, `sphere`, `turned` | 289 | 289 | 1.1 to 10 ms |
| `coincident`, `tangent`, `nearmiss`, `chain`, `random`, `features` | 329 | 329 | 1 to 35 ms |
| `brep-*` (19 categories) | 236 | 236 | 0.4 to 127 ms |
| `step` | 280 | 280 | 185 ms |
| `step-conformance` | 108 | 105 | 65 ms |
| `step-import` | 675 | 663 | 12 ms |

- The three wrong are the freeform panel of step-conformance at 0.0125, 0.05 and 0.1 mm: 0.48 to 0.55
  mm off its true surface.
- The 12 refused are STEP files, each with its reason in the run's output: a solid invalid as written
  (two faces overlap by 4.7e-7), a face that goes several times around its axis (2), no solid in the
  file (2), an edge 0.04 off its surface (2), a loop that does not close on its surface (2), a face with
  no axis, edges that do not join two faces, a loop around an axis that never comes by its seam.
- Every boolean of every file that imports passes.

A boolean that cannot be done exactly is refused, not approximated. Where four faces of a result come to
one edge, each two that bound the same material get a copy of the edge of their own, and the result is
a valid solid. A result is tidied before it is handed back: faces side by side in one surface become one
face, and edges end to end on one curve one edge. The boolean code has one path for every shape:
`lib/boolean` holds only NURBS curves and surfaces and finds where they meet by Bézier clipping, with no
case for lines, planes, cylinders or any other kind.

The STEP importer reads solids, at every place an assembly uses them (tessellated AP242 solids too), in
any length unit; it does not read wires. IGES is read and written
(`shapecraft_ops_step::{import_iges, export_iges}`) as trimmed NURBS surfaces, with a round trip test
(`bodies_written_as_iges_read_back_as_they_were`).
