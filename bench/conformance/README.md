# ShapeCraft on brep-conformance

[brep-conformance](https://github.com/springcraft-ai/brep-conformance) is a kernel-neutral suite
published with the kernel [springrep](https://github.com/springcraft-ai/springrep). It drives a kernel
through an adapter (one JSON request per line on stdin, one reply per line on stdout) and judges every
case at 1e-7 mm, and again at 1e-6, 1e-5 and 1e-3 mm. A kernel's headline is the share of all cases it
passes at 1e-7 mm; cases it does not support count against it.

This folder holds ShapeCraft's adapter (`src/`, the binary `shapecraft-adapter`) and the scripts that
build and run the suite in a sandbox. How the results compare with other kernels' is in
[`docs/benchmarks`](../../docs/benchmarks/README.md).

## Method

The suite's rule is that an adapter answers each function with the kernel's one call for it and adds
nothing: no composing of functions, no retries, no repair, no thresholds. A function without such a call
is left out of `supports`, and its cases count as unsupported. The suite's maintainers run kernels they
have registered, with adapters they review; ShapeCraft is not registered, so a run here is
self-scored and its adapter has not been reviewed by them.

What the adapter calls:

| Function | ShapeCraft's call |
|---|---|
| `box`, `cylinder`, `sphere`, `torus`, `cone`, `slab` | The operations `AddBox`, `AddCylinder`, `AddSphere`, `AddTorus`, `AddCone`; `slab` is built in `src/main.rs` |
| `extrude`, `revolve` | A `Sketch` of the profile, then `Extrude` or `Revolve` |
| `screw` | A `Sketch`, then `SweptLoft` as a coil |
| `loft`, `loft_through`, `sweep`, `sweep_path` | `shapecraft_shape::swept::through_sections`, `along_path` |
| `union`, `intersection`, `difference`, `union_all`, `difference_all` | `shapecraft_boolean::combine`, `combine_all` |
| `fillet`, `fillet_variable`, `fillet_conic`, `fillet_g2`, `fillet_setback`(+`_g2`), `fillet_radii`, `fillet_faces`, `full_round`, `chamfer`, `chamfer_two`, `chamfer_distances` | `shapecraft_shape::blend_chains`, `blend_solid` and the blend library's variants; corners closed |
| `offset`, `shell`, `offset_faces`, `move_faces`, `thicken`, `draft` | `thick::thickened`, `faces_offset`, `faces_moved`, `thicken`, `drafted` |
| `delete_faces` | `shapecraft_ops_faces::faces_deleted` |
| `split`, `transform`, `affine` | `shapecraft_shape::halves`, `Solid::mapped`, `Solid::sheared` |
| `write_step`, `read_step`, `sew` | `shapecraft_ops_step::export`, `import`, `import_sheets` with `shapecraft_boolean::stitched` |
| `volume`, `area`, `centroid` | `shapecraft_boolean::measure` |
| `classify`, `normal`, `closest`, `curvature`, `bounding_box`, `raycast` | `Placer` (`shapecraft_boolean::place`) |
| `section`, `project`, `display`, `interference`, `distance` | `plane_section` and `shapecraft_shape::section`, `project`, `display`, `interference`, `distance` |
| `check` | `shapecraft_boolean::write` into a fresh `Topo`, then `Topo::validate` |

Not offered: `inertia`, `betti`, `section_area`, `mesh`.

Choices the adapter makes, within the rule:

- **`hello` declares a tolerance of 1e-7 mm and no configuration.** The kernel's tolerances are relative to
  a solid's size; nothing is set.
- **Every kernel error is answered as `failed`**, because ShapeCraft's errors do not say whether the input or
  the kernel is at fault. Only the adapter's own "this point names no edge/face" is `invalid`.
- **A point names an edge** through `geometry::on_curve` at 1e-6 mm, and a face through `lies_on` at 1e-6
  mm. A seam (an edge both of whose uses are by one face) is not counted as an edge, as the suite's own
  definition of `display` lists seams apart from edges. A point that still lies on two edges or two faces
  is answered `unsupported`, as the rule says.
- **`read_step` joins the solids of a file with `combine(Union)`**, as the contract asks.
- **Profiles.** Lines, arcs and conics are drawn as the sketch's lines, arcs and rational quadratic
  splines; a periodic spline is given as the same curve with clamped knots (`profile.rs`). A `gn` blend
  section is not a NURBS: `unsupported`.
- **`loft`, `loft_through`, `sweep` take one outline.** A profile with a hole is `unsupported` there.
- **`move_faces` with a rotation** is `unsupported`: `faces_moved` only shifts.

## Running it

Files:

| Path | What |
|---|---|
| `build.sh` | Builds the adapter into `target/conformance` (`--head`: against the last commit) |
| `sandbox.sh` | The `bwrap` line: no network, `/home` hidden, one folder writable |
| `run.sh` | Runs the suite on `shapecraft` or `springrep` inside the sandbox |
| `summarize.py` | The tables, and the list of failing cases, from results files |

Where things live: `SHAPECRAFT_BENCH_DIR` (default `~/.cache/shapecraft-bench`) holds the clones
(`brep-conformance`, `brep-conformance-springrep`) and the sandbox `conformance-sandbox/` with the
suite, springrep's adapter, the fetched crates and the results in `out/`. Nothing in it is part of the
repository.

```sh
# 1. ShapeCraft's adapter (our code, built outside the sandbox, into target/conformance)
bench/conformance/build.sh

# 2. the whole suite, in the sandbox; about a minute with 14 workers
OUT=shapecraft THREADS=14 bench/conformance/run.sh shapecraft
#    one area: a test path, a directory, or a family's name
OUT=try bench/conformance/run.sh shapecraft --family fillets/ --timeout 60

# 3. the tables and the failures
python3 -I bench/conformance/summarize.py $SHAPECRAFT_BENCH_DIR/conformance-sandbox/out/shapecraft.json
python3 -I bench/conformance/summarize.py --failures $SHAPECRAFT_BENCH_DIR/conformance-sandbox/out/shapecraft.json

# springrep (the maintainers' adapter, for comparison on this machine)
bench/conformance/run.sh springrep
```

`SUITE` and `SPRINGREP_ADAPTER` name the sandbox's folders for the suite and springrep's adapter
(defaults `suite` and `springrep-adapter`), to keep several versions side by side. The suite's HTML
report is `out/<name>.html`; it holds its own data and asks the network for nothing.

`run.sh` is this line, with the paths filled in:

```sh
bwrap --unshare-all --die-with-parent \
  --ro-bind /usr /usr --symlink usr/lib /lib --symlink usr/lib64 /lib64 --symlink usr/bin /bin \
  --ro-bind /etc/alternatives /etc/alternatives --ro-bind /etc/ld.so.cache /etc/ld.so.cache \
  --proc /proc --dev /dev --tmpfs /tmp --tmpfs /home --tmpfs /run \
  --ro-bind ~/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu /toolchain \
  --bind $SHAPECRAFT_BENCH_DIR/conformance-sandbox /work \
  --ro-bind $SHAPECRAFT_BENCH_DIR/conformance-sandbox/cargo-home /work/cargo-home \
  --ro-bind target/conformance/release /shapecraft-bin \
  --clearenv --setenv PATH /toolchain/bin:/usr/bin --setenv HOME /tmp \
  --setenv CARGO_HOME /work/cargo-home --setenv CARGO_NET_OFFLINE true --setenv LANG C.UTF-8 \
  --chdir /work \
  sh -c 'cd /work/out && exec /work/suite/target/release/brep-conformance \
    --adapter /shapecraft-bin/shapecraft-adapter --threads 14 --out shapecraft.json --html shapecraft.html > shapecraft.log 2>&1'
```

The suite spawns the adapter, so ShapeCraft's adapter runs in the same sandbox, its binary mounted
read-only. `sandbox.sh` expects the Rust toolchain 1.99.1 in `~/.rustup/toolchains`; change `TC` in it
for another.

### Setting the sandbox up from nothing

The suite at the commit of its official 2.0.1 run, and springrep's adapter at the commit the suite's
registry pinned for that run:

```sh
export SHAPECRAFT_BENCH_DIR=$HOME/.cache/shapecraft-bench
mkdir -p $SHAPECRAFT_BENCH_DIR && cd $SHAPECRAFT_BENCH_DIR
git clone https://github.com/springcraft-ai/brep-conformance
git -C brep-conformance checkout dc7ac9ed1e588978767a45306f098f6f512f8bba
git clone https://github.com/springcraft-ai/brep-conformance-springrep
git -C brep-conformance-springrep checkout feaf7ef2b9231f4f22b1992031cd7a1aa414ea30
SB=$PWD/conformance-sandbox; mkdir -p $SB/out $SB/cargo-home
rsync -a --exclude .git brep-conformance/ $SB/suite/
rsync -a --exclude .git brep-conformance-springrep/ $SB/springrep-adapter/
rm $SB/suite/rust-toolchain.toml $SB/springrep-adapter/rust-toolchain.toml   # they pin 1.99.0; use the installed 1.99.1
# in $SB/springrep-adapter/Cargo.toml: delete the [dev-dependencies] section (a git dependency
# only its tests use) and add an empty [workspace] table (so that it builds on its own)
(cd $SB/springrep-adapter && CARGO_HOME=$SB/cargo-home RUSTUP_TOOLCHAIN=1.99.1 cargo fetch)
(cd $SB/suite && CARGO_HOME=$SB/cargo-home RUSTUP_TOOLCHAIN=1.99.1 cargo fetch --locked)
cd - && bench/conformance/sandbox.sh -- sh -c 'cd /work/suite && cargo build --release --offline --locked -p brep-conformance'
bench/conformance/sandbox.sh -- sh -c 'cd /work/springrep-adapter && cargo build --release --offline'
```

`cargo fetch` only downloads. After the first build inside the sandbox, never run cargo outside it with
`CARGO_HOME=$SB/cargo-home`; the sandbox mounts that folder read-only for the same reason.

springrep is run from **springrep 2.1.0 on crates.io**, which the adapter's manifest asks for
(`springrep = "=2.1.0"`), not from a clone of its repository.

The suite also has cases that read NIST's STEP files (`step/read-nist`, 33 of its 3650 cases); they
need its `nist/fetch.py` to download the files, which is not run here, so they are not part of the
results below.

## Audit of the third-party code

Done on 2026-11-22 against the suite 2.0.0 and springrep 2.0.0 clones, the adapter's repository, and the
crates.io copies that were compiled; not repeated for the 2.0.1 / 2.1.0 versions run now, apart from
building them in the same sandbox. Verdict then: nothing malicious found. Everything of theirs was still
built and run only inside the sandbox.

- **Build scripts.** springrep has one `build.rs`: it reads the `.rs` files under `src`, hashes them
  (FNV-1a) and sets the environment variable `SPRINGREP_SOURCE` for the compiler. It writes nothing and
  starts nothing. The suite's three crates and the adapter have none.
- **Network.** No socket, HTTP client or URL fetch in any Rust source. The only URLs are a CDN prefix
  written into reports and the registry's GitHub addresses.
- **Processes.** `brep-protocol` starts the adapter it is given and kills its process group on a timeout
  (two `unsafe` lines: `libc::kill`). `suite/src/registry.rs` runs `git clone` and `sh -c <build step>`
  from `kernels.json`, but only for `--kernel NAME`; the runs here use `--adapter` and never reach it.
  Do not use `--kernel` outside the sandbox: it downloads and builds other people's code.
- **Files.** The suite writes its `--out`, `--html`, `--glb` and `--step` outputs where it is told;
  springrep's library writes no file.
- **Embedded files.** A minified three.js r128 goes into the HTML report; it runs only in a browser that
  opens the report.
- **Scripts not run.** `bench/*.sh` (their benchmark machine's setup), `nist/fetch.py`, `site/`, the
  GitHub workflows.

## Current results

2026-11-25, suite 2.0.1 at commit `dc7ac9e` (3617 cases without the NIST ones, seed 1, 120 s per call, 14
workers on a 32-thread AMD Ryzen 9 7950X shared with other jobs), ShapeCraft at `450425c` with
uncommitted changes. **All 3617 cases pass at 1e-7 mm; no failure, refusal, timeout or crash.** The
whole run took 55 s and 68 s in two runs.

| category | cases | at 1e-7 mm | median ms (two runs) |
|---|---|---|---|
| booleans | 1253 | 1253 | 47.9 / 86.9 |
| constructions | 448 | 448 | 131.2 / 133.6 |
| queries | 320 | 320 | 0.5 / 0.7 |
| modify | 612 | 612 | 5.9 / 6.2 |
| fillets | 627 | 627 | 4.3 / 4.5 |
| chamfers | 171 | 171 | 1.8 / 1.9 |
| step | 130 | 130 | 0.4 / 0.5 |
| parts | 56 | 56 | 14.4 / 18.2 |
| all | 3617 | 3617 | 22.1 / 28.1 |

springrep 2.1.0 passes all 3617 as well on this machine (57 s and 71 s for the whole run). The times are
from a machine with other work running, and differ between runs, so they are not a measure of speed.
