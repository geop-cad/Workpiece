# Benchmarks

Workpiece is measured with three benchmarks that other people wrote or that are public data. This
page says what each one is, where it comes from, how to reproduce Workpiece's numbers, what they
are, and what other projects' numbers are known and from where. The numbers are in
[`results.json`](https://github.com/geop-cad/Workpiece/blob/main/docs/benchmarks/results.json), which a script regenerates from a run
([below](#results-json-and-ci)).

## Summary

<!-- ANCHOR: summary -->
| Benchmark | What it grades | Workpiece | Scored by |
|---|---|---|---|
| [brep-conformance](#brep-conformance) 2.0.1 | all 3650 cases, the 33 that read NIST's STEP files included: booleans, constructions, queries, modify, fillets, chamfers, STEP, parts, at 1e-7 mm | 3650 of 3650 | Workpiece (self-scored; the suite's authors have not reviewed Workpiece's adapter) |
| [The boolean bench](#the-boolean-bench) | 1917 cases: closed-form booleans, near misses, random pairs, brep-bench's cases, 675 public STEP files | 1902 pass, 12 refused, 3 wrong, 0 crashed | Workpiece (self-scored) |
| [step-conformance](#step-conformance) | 27 STEP parts at four mesh tolerances (108 cells) | 105 of 108 | Workpiece's port of the benchmark's rule (self-scored); not the benchmark's own scorer |

Run on 2026-11-25 (brep-conformance: the whole suite again on 2026-11-26). On brep-conformance,
springrep also passes every case (the maintainers' run of all 3650, and reproduced here on the 3617
without NIST's files), cadrs passes 3648 of 3650, and the other kernels in the
maintainers' published runs pass fewer. On step-conformance, Workpiece is level with three published
readers at 105 cells, and one published reader (ogeom) passes two more. Details, and where Workpiece is
weak, follow.
<!-- ANCHOR_END: summary -->

## What the benchmarks are

### brep-conformance

[brep-conformance](https://github.com/springcraft-ai/brep-conformance) is a kernel-neutral
conformance suite for B-rep kernels, published with the kernel
[springrep](https://github.com/springcraft-ai/springrep). It drives a kernel through an adapter (one
JSON request per line on stdin, one reply per line on stdout) and judges each case at 1e-7 mm; a
kernel's headline is the share of all cases it passes at that tolerance, and cases a kernel does not
support count against it. Its categories: `booleans` (1253 cases), `constructions` (448), `queries`
(320), `modify` (612), `fillets` (627), `chamfers` (171), `step` (163, of which 33 read NIST's STEP
files), `parts` (56).

Workpiece's adapter is in [`bench/conformance`](https://github.com/geop-cad/Workpiece/blob/main/bench/conformance). Per the suite's rule it
answers each function with the kernel's one call for it and adds nothing. The suite's maintainers
publish results for the kernels in their registry, each run by them on one bare-metal machine; their
runs of a kernel use an adapter they reviewed. Workpiece is not in their registry, so its row is
**self-scored**: made here, with an adapter they have not reviewed.

### The boolean bench

[`bench/boolean`](https://github.com/geop-cad/Workpiece/blob/main/bench/boolean) is Workpiece's own suite for its booleans and STEP importer. It
is not a third-party benchmark; it is here because it uses public data and outside case sets, and
everything in it can be rerun. Each case is graded as in brep-bench: **pass** (a valid closed solid,
volume and area to a relative 1e-6, measured from the surfaces, not from a mesh), **refused** (the
boolean declined, with a reason), **wrong**, or **crashed**. Its cases:

- hand-made cases with closed-form answers (`box`, `cylinder`, `sphere`, `turned`, `coincident`,
  `tangent`, `nearmiss`, `chain`, `random`, `features`);
- the boolean, tangent, fillet, chamfer, geometry, parts, NURBS, spline, singular, primitives, sweep,
  loft, swept, feature, edit, offset, shell, thicken and draft cases of
  [brep-bench](https://github.com/bmander/brep-bench) (MIT OR Apache-2.0; its results are at
  [brepbench.org](https://brepbench.org)), as of its snapshot of early November 2026, built by
  Workpiece's own port of each case and graded by brep-bench's rule;
- 675 STEP files that must import as valid closed solids (`step-import`), and parts of them halved,
  drilled and combined (`step`).

The STEP data is fetched by `boolean-bench fetch` and pinned by SHA-256 in
[`datasets.tsv`](https://github.com/geop-cad/Workpiece/blob/main/bench/boolean/datasets.tsv): the CTC, FTC and STC parts of
[NIST's MBE PMI validation and conformance set](https://www.nist.gov/el/systems-integration-division-73400/mbe-pmi-validation-and-conformance-testing/download-free-cad),
samples of the [KiCad 3D model library](https://gitlab.com/kicad/libraries/kicad-packages3D)
(CC-BY-SA 4.0 with an exception), of the [FreeCAD library](https://github.com/FreeCAD/FreeCAD-library)
(CC-BY 3.0), and the parts of step-conformance (CC0).

### step-conformance

[step-conformance](https://github.com/dan335/step-conformance) (code under MIT or Apache-2.0, parts CC0) measures
STEP importers: 27 parts (7 shop parts, a freeform panel, 19 generated parts of known shape), each
read and meshed at 0.0125, 0.05, 0.1 and 0.5 mm, 108 cells. A cell passes if the mesh has every face
of the file, reaches no further than the file's geometry, and is within four tolerances of the true
surface both ways, in under 60 seconds. Its maintainers publish a table of readers (below).

Workpiece's cells are scored by `bench/boolean/src/conformance.rs`, Workpiece's own implementation
of that rule, not by the benchmark's scorer. For the shop parts the benchmark's reference is
OpenCascade's fine mesh; OpenCascade is not run here, so Workpiece's own fine mesh stands in. Those
28 cells therefore check Workpiece against itself and against the file's facts (every face, nothing
outside the file's geometry, every vertex reached), which is weaker than the benchmark's check. The
other 80 cells have an exact truth and are scored as the benchmark scores them. (Workpiece was run
through the benchmark's own scorer once, on 2026-11-13, at commit 6693c37 of step-conformance, which
gave the same 105; that run is not repeated here.)

## Workpiece's results

The runs of 2026-11-25, and the whole brep-conformance suite again on 2026-11-26 (below). Machine: AMD Ryzen 9 7950X (16 cores, 32 threads), 124 GB, Ubuntu 22.04.5,
Linux 6.8.0; the machine was shared with other jobs (load average 6 to 28), so times are indicative.
Workpiece was built from commit `450425c` plus uncommitted changes (`rustc 1.97.0-nightly`, release
profile). The suite and springrep were built with Rust 1.99.1 in the sandbox described below.

### brep-conformance

Suite 2.0.1 at commit `dc7ac9e`, the one the maintainers' runs below were made on. The whole suite, all
3650 cases, in one run on 2026-11-26 with `--nist` (the 33 cases that read NIST's STEP files are opt-in
in the suite; the files were downloaded with the suite's own checksum-verified `nist/fetch.py`), seed
1, 120 s per call, 14 workers, judged at 1e-7 mm: **3650 of 3650**: booleans 1253, constructions 448,
queries 320, modify 612, fillets 627, chamfers 171, step 163 (the 33 NIST cases included), parts 56;
none unsupported, refused, timed out, failed or crashed. Workpiece does not answer four functions
(`inertia`, `betti`, `section_area`, `mesh`); no case was counted as unsupported for lacking one.

The two earlier runs of 2026-11-25, without the NIST cases (3617 cases), gave 3617 of 3617; the
medians per category below are from those two runs, made back to back with springrep in the same
sandbox:

| category | cases | Workpiece at 1e-7 mm | median ms, run 1 / run 2 |
|---|---|---|---|
| booleans | 1253 | 1253 | 47.9 / 86.9 |
| constructions | 448 | 448 | 131.2 / 133.6 |
| queries | 320 | 320 | 0.5 / 0.7 |
| modify | 612 | 612 | 5.9 / 6.2 |
| fillets | 627 | 627 | 4.3 / 4.5 |
| chamfers | 171 | 171 | 1.8 / 1.9 |
| step (without NIST) | 130 | 130 | 0.4 / 0.5 |
| parts | 56 | 56 | 14.4 / 18.2 |
| all | 3617 | 3617 | 22.1 / 28.1 |

No case failed, was refused, timed out or crashed. A whole run of the 3617 took 55 s and 68 s of wall time
(the run of all 3650: 53 s).
springrep 2.1.0 (the maintainers' adapter, reproduced here in the same sandbox, back to back) took 57 s
and 71 s and passed all 3617 as well; its medians in the same two runs were: booleans 44.5 / 44.8,
constructions 166.6 / 177.8, queries 1.1 / 1.2, modify 2.2 / 2.2, fillets 3.4 / 3.5, chamfers 1.2 / 1.3,
step 0.5 / 0.5, parts 14.2 / 14.6, all 11.8 / 11.9. On this machine Workpiece's median was lower in
constructions and queries and higher in modify, fillets and chamfers, and the all-case median was higher.
Neither kernel is faster in every category, and the two runs differ by up to a factor of two for
Workpiece's booleans, so this is not a speed claim.

### The boolean bench

`target/release/boolean-bench`, 1917 cases, 23.6 s of wall time on 32 threads: **1902 pass, 12 refused,
3 wrong, 0 crashed.**

| Group | Cases | Pass |
|---|---|---|
| Hand-made (`box`, `cylinder`, `sphere`, `turned`, `coincident`, `tangent`, `nearmiss`, `chain`, `random`, `features`) | 618 | 618 |
| brep-bench's cases (`brep-*`, 19 categories) | 236 | 236 |
| `step`: parts of public files, halved, drilled, combined | 280 | 280 |
| `step-import`: 675 public STEP files | 675 | 663 (12 refused) |
| `step-conformance` (below) | 108 | 105 (3 wrong) |

The 12 refused files are listed with their reasons in the bench's output: a solid invalid as written
(two faces overlap by 4.7e-7), a face that goes several times around its axis (2), files with no solid
(2), an edge 0.04 off its surface (2), a loop that does not close on its surface (2), a face with no
axis, edges that do not join two faces, and a loop around an axis that never comes by its seam. The
boolean bench's per-category medians are in `results.json`.

brep-bench's cases are built by Workpiece's own port (`bench/boolean/src/brepbench.rs`) and graded by
brep-bench's rule against the expected values copied from its case files, as of its snapshot of
2026-11-05. Workpiece is not on brep-bench's published board, and brep-bench has more cases now, so no
other kernel's numbers are put beside these.

### step-conformance

Workpiece's cells, by kind of part, beside the table the benchmark's maintainers published on
2026-11-22 (their README at commit `3f6d18b`, `results/RESULTS.md` at `61b6a87`; one Intel Core
i7-12700KF under Windows; the other readers were run by them, not here):

| Reader | Version | Shop parts (28) | Freeform panel (4) | Known shape (76) | All (108) | Scored by |
|---|---|---|---|---|---|---|
| Workpiece (run here 2026-11-25) | `450425c` + changes | 28 | 1 | 76 | **105** | Workpiece's port of the rule (self) |
| [ogeom](https://github.com/gilbertorconde/ogeom-rs) | 0.11.5 | 28 | 3 | 76 | 107 | benchmark maintainers |
| [OpenCascade](https://github.com/Open-Cascade-SAS/OCCT) | opencascade-rs 0.3.0, OCCT 7.8.1 | 28 | 1 | 76 | 105 | benchmark maintainers |
| [solidcraft](https://github.com/tangency-labs/solidcraft) | c95c2bef4 | 28 | 1 | 76 | 105 | benchmark maintainers |
| solidcraft through [cadmpeg](https://github.com/cadmpeg/cadmpeg) | c95c2bef4, cadmpeg 0.6.12 | 28 | 1 | 76 | 105 | benchmark maintainers |
| [truck](https://github.com/ricosjp/truck), master | 88ed005 | 23 | 0 | 68 | 91 | benchmark maintainers |
| truck, released | truck-stepio 0.3.0, truck-meshalgo 0.4.0 | 21 | 0 | 60 | 81 | benchmark maintainers |
| [brepkit](https://github.com/andymai/brepkit) | 4.1.49 | 20 | 0 | 30 | 50 | benchmark maintainers |

The three cells Workpiece does not pass are the freeform panel at 0.0125, 0.05 and 0.1 mm, 0.48 to
0.55 mm off the true surface; the benchmark's README says its OpenCascade reader is also 0.6 mm off
there. Workpiece's median for a cell is 65 ms on this machine; the benchmark's
own timings are on another machine and are not compared.

## Other kernels

Only numbers that their benchmark's maintainers published, or that were reproduced here, are given.
Workpiece did not run any other kernel for this page except springrep (below).

### brep-conformance, suite 2.0.1

The maintainers' official run on 2026-11-23: one bare-metal AWS `c7i.metal-24xl` (Intel Xeon
Platinum 8488C), suite 2.0.1 at commit `dc7ac9e`, all 3650 cases. The reports are on the suite's CDN
(run `45083601338-1`; kentron `45204786615-1`; anvil-brep and springrep `45259371420-1`; listed in
the suite's [`reports.json`](https://github.com/springcraft-ai/brep-conformance/blob/b44cf4f/reports.json));
the numbers below are counted from those files by
[`update_results.py`](https://github.com/geop-cad/Workpiece/blob/main/docs/benchmarks/update_results.py). The leaderboard is at
[springcraft-ai.github.io/brep-conformance](https://springcraft-ai.github.io/brep-conformance/).
Cases passed at 1e-7 mm, best first (by the share of the cases run that passed, the `all` column;
ties by the number passed):

| Kernel | Version | Licence | booleans (1253) | constructions (448) | queries (320) | modify (612) | fillets (627) | chamfers (171) | step (163) | parts (56) | all (3650) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| [springrep](https://github.com/springcraft-ai/springrep) | 2.1.0 | MIT | 1253 | 448 | 320 | 612 | 627 | 171 | 163 | 56 | 3650 |
| **Workpiece** (self-scored, run here 2026-11-26) | 0.1.0 | MIT OR Apache-2.0 | 1253 | 448 | 320 | 612 | 627 | 171 | 163 | 56 | **3650** |
| [cadrs](https://github.com/rvdende/cadrs) | 0.2.9 | MIT OR Apache-2.0 | 1253 | 448 | 320 | 612 | 627 | 171 | 161 | 56 | 3648 |
| [kentron](https://github.com/rsz269/kentron) | 0.19.3 | AGPL-3.0-or-later AND BSD-3-Clause AND MIT | 1183 | 410 | 316 | 535 | 582 | 171 | 148 | 55 | 3400 |
| [OpenCascade](https://github.com/Open-Cascade-SAS/OCCT) | 8.0.1.1.0 | LGPL-2.1-only WITH OCCT-exception-1.0 | 1192 | 279 | 270 | 421 | 418 | 171 | 142 | 40 | 2933 |
| [next.BREP](https://github.com/mmiscool/next.BREP.io_RUST_BREP_KERNEL) | 0.24.0 | LicenseRef-Autodrop3d | 995 | 300 | 60 | 344 | 343 | 129 | 140 | 34 | 2345 |
| [arris](https://github.com/arris-labs/arris) | 0.10.0 | MIT OR Apache-2.0 | 962 | 223 | 213 | 184 | 328 | 115 | 128 | 29 | 2182 |
| [cad-kernel](https://github.com/bmander/geomsolver) | 0.17.0 | MIT OR Apache-2.0 | 1165 | 194 | 53 | 347 | 220 | 0 | 0 | 30 | 2009 |
| [solidcore](https://github.com/tangency-labs/solidcore) | 1.0.1 | Apache-2.0 | 958 | 267 | 60 | 54 | 359 | 0 | 0 | 29 | 1727 |
| [anvil-brep](https://github.com/swperb/anvilcad) | 0.2.0 | MIT OR Apache-2.0 | 811 | 128 | 0 | 184 | 118 | 96 | 0 | 9 | 1346 |

How to read it: every row but Workpiece's is the maintainers' report, whole suite (3650 cases);
Workpiece's row is the run here of the same 3650 cases (the NIST files included), self-scored.
springrep 2.1.0 was also reproduced here on the 3617 cases without the NIST files: 3617 of 3617. The maintainers' registry lists a version for
each kernel and the adapter commit their run used; some kernels' licences are as their registry
states and may differ in other releases. The rows say what each kernel passed in the suite's run, not
what it can do outside the suite.

### step-conformance

See the table [above](#step-conformance-1): its other rows are the maintainers' own.

## Where Workpiece is weak

Taken from the results above and from the code and [`README.md`](https://github.com/geop-cad/Workpiece/blob/main/README.md) (what is not built is
listed per feature in [`Ribbon.md`](https://github.com/geop-cad/Workpiece/blob/main/Ribbon.md)):

- **Self-scored.** Every Workpiece row is a run by Workpiece's authors. The brep-conformance suite's
  authors have not reviewed Workpiece's adapter, and the step-conformance maintainers have not
  scored this version.
- **Functions not answered.** The brep-conformance adapter does not answer the functions `inertia`,
  `betti`, `section_area` and `mesh` (no case of the suite's 3650 needed them).
- **STEP import.** 12 of 675 public STEP files are refused (listed above); the importer reads solids
  (including tessellated AP242 solids) and does not read wires, and refuses a band around a B-spline
  ring's tube (`ops/step/src/lib.rs`); 3 of 108 step-conformance cells fail (the freeform panel); on the
  shop parts of step-conformance Workpiece is compared with itself.
- **Booleans** that cannot be done exactly are refused with a reason instead of approximated; the
  bench counts a refusal as better than a wrong answer, and has 12 refusals, all of STEP files.
- **Speed.** On brep-conformance, springrep's per-category medians are lower than Workpiece's in
  modify, fillets and chamfers on the same machine in the same sandbox.
- **Features not built** are listed in `Ribbon.md` (for example guide rails for sweeps, ellipse
  constraints, sheet-metal reliefs, hems and jogs).
- **Benchmarks are narrow.** All three measure geometry kernels on solids; none measures the sketcher,
  assemblies, drawings or the application.

## Reproducing Workpiece's numbers

You need a Rust toolchain (the workspace's `rust-version` is 1.95), Python 3, and `bubblewrap`
(`bwrap`) for the suite. Every command is run from the repository root.

### The boolean bench and step-conformance

```sh
cargo build --release -p workpiece-boolean-bench
target/release/boolean-bench fetch            # once: downloads the pinned datasets (about 270 MB)
target/release/boolean-bench | tee boolean.log   # about half a minute on 32 threads; minutes on fewer
# the per-case results are in target/bench-results/boolean.tsv
```

`boolean-bench step-conformance` runs only that benchmark's cells. The run fails if a case that
passes in `bench/boolean/baseline.tsv` no longer does.

### brep-conformance

The suite is third-party code, so it is built and run in a sandbox with no network and no access to
your home directory; [`bench/conformance/README.md`](https://github.com/geop-cad/Workpiece/blob/main/bench/conformance/README.md) has the whole
setup. In short:

```sh
export WORKPIECE_BENCH_DIR=$HOME/.cache/workpiece-bench
# once: clone the suite and the adapter at the pinned commits, fetch their crates, build them in the sandbox
# (bench/conformance/README.md, "Setting the sandbox up from nothing", has the commands: the suite at
# commit dc7ac9ed1e588978767a45306f098f6f512f8bba and springrep's adapter at feaf7ef2b9231f4f22b1992031cd7a1aa414ea30)

bench/conformance/build.sh                       # Workpiece's adapter, outside the sandbox
OUT=workpiece THREADS=14 bench/conformance/run.sh workpiece
python3 -I bench/conformance/summarize.py $WORKPIECE_BENCH_DIR/conformance-sandbox/out/workpiece.json
```

The suite's `--threads` and the machine change the times, not the passes. springrep was run with `bench/conformance/run.sh springrep`
(the adapter's repository at commit `feaf7ef`, which depends on `springrep = "=2.1.0"` from crates.io).

### Regenerating `results.json`

```sh
python3 -I docs/benchmarks/update_results.py \
  --conformance   $WORKPIECE_BENCH_DIR/conformance-sandbox/out/workpiece.json \
  --boolean-log   boolean.log --boolean-tsv target/bench-results/boolean.tsv \
  --machine "describe your machine" --commit "$(git rev-parse --short HEAD)"
```

Add `--reproduced FILE` for springrep's results file, and `--published-conformance FILE...` for the
maintainers' reports (download them from the CDN paths in the suite's `reports.json`:
`https://cdn.springcraft.ai/brep-conformance/reports/<run>/<file>`). The script keeps the rows it was
not given inputs for.

## `results.json` and CI

[`results.json`](https://github.com/geop-cad/Workpiece/blob/main/docs/benchmarks/results.json) has, per benchmark, the rows above: kernel, version, licence, link, who
scored it, the date, pass and total overall and by category, and times where there are some. A page can
draw a chart from it directly (the homepage section links to this page's [Summary](#summary)).
[`.github/workflows/bench.yml`](https://github.com/geop-cad/Workpiece/blob/main/.github/workflows/bench.yml) runs Workpiece's own rows (the
boolean bench with step-conformance, and brep-conformance with the suite pinned at `dc7ac9e`) on
manual dispatch and weekly, and uploads a regenerated `results.json` as an artifact. It does not run any
other kernel. The runner is a shared virtual machine; its times are not comparable with the ones here.

## Method notes

- A row's "published" origin means the numbers come from files or tables the benchmark's maintainers
  published, copied or counted without change. "Reproduced" means built and run here from the pinned
  public version in the sandbox. "Run here" is Workpiece.
- Other kernels are named with the version and licence their benchmark states, and linked. No row is
  ranked or described beyond its numbers.
- Third-party code (the suite, springrep and its adapter) was built and run only inside `bwrap` with no
  network, with the crates fetched beforehand and not built outside the sandbox.
- A kernel missing from a table was not run by its benchmark's maintainers or was not reproducible here.
  Of the kernels Workpiece's authors have run before in other benchmarks (brep-bench cases, STEP
  rounds) no result is given on this page, because the cases have changed since, or the run cannot be
  repeated with a command given here.
