# Architecture

A map of the crates and of how a part is built, for someone who wants to find their way in the code. Everything here was read from the code; the crate-level doc comments (`//!` at the top of each `lib.rs`) say more, and `cargo doc --workspace --no-deps --open` shows them.

## The idea

A **part** is a program: an ordered list of **operations** (also called steps) with arguments. Applying the operations in order to an empty **world** gives the part's geometry. The program, the parameters and a cache of solved values are what is saved in a `.wpc` file (JSON); geometry is never saved, it is rebuilt.

A **step never depends on a later step**: a step sees only what the steps before it made. Across parts, a part never contains itself; building a part that is already being built fails.

## Crates

Dependencies point downwards in this list.

| Layer | Crate | Role |
|---|---|---|
| core | `core` (`workpiece-core`) | The world, references, the `Operation` trait, the `Builder` trait, the `Cache`, the `Registry`, widgets and handles (what a dialog and the viewport show while a step is edited), units, measures with formulas, rigid poses. |
| core | `core/derive` | `#[derive(Arguments)]`: from the fields of an operation's arguments it finds what they refer to and makes the default dialog. |
| lib | `lib/boolean` | Booleans of solids over tolerant NURBS B-reps: finds where boundaries meet, cuts the faces, keeps the pieces. A boolean it cannot do exactly is a `Refusal` with a reason. |
| lib | `lib/shape` | Geometry that several operations share: sweeps and lofts, blends (fillet, chamfer), thickening, shelling, drafting, healing, surfaces, splitting, threads. |
| lib | `lib/solver` | The geometric constraint solver of sketches: Newton iteration with exact derivatives, least-motion solutions, diagnosis of degrees of freedom, redundant and conflicting constraints. |
| lib | `lib/render` | A small path tracer for the Render step. |
| ops | `ops/*` | One crate per family of operations. Each has a `register` function that tells the registry its operations. See below. |
| cad | `cad/cad-part` | `Part`, `Workspace` (parts by path), `Engine` (builds parts, with caching), the thread pool, naming fallback, material library, properties. |
| cad | `cad/cad-ui` | The application as data: `Model` (all state), `View` (derived from the model each frame), `Message` (what the view asks for). No window. |
| cad | `cad/cad-rasterize` | Turns built worlds and edit handles into triangle, line and point buffers for drawing and picking. |
| host | `hosts/app` | The desktop application: egui for the interface, OpenGL (`glow`) for the viewport. Reads and writes files; the model does not. The same crate compiled for `wasm32-unknown-unknown` is the application in the browser (`src/web.rs`, built by `website/app/build.sh`). |
| host | `hosts/cli` | The `workpiece` command: `make`, `report`, `kinds`, `materials`. |
| bench | `bench/boolean`, `bench/conformance` | Benchmarks, outside `cargo test`. |

The operation crates (`ops/`): `basic-shapes`, `sketch`, `sweep`, `features` (holes, threads), `blend`, `faces`, `bodies`, `datum`, `surface`, `sheet-metal`, `annotate`, `drawing`, `appearance`, `mesh`, `step` (STEP, IGES, STL and 3MF in, STEP, IGES, STL and 3MF out), `instances` (placing parts, joints), `standard` (screws, nuts, washers, bearings). `ops/testing` is only for their tests.

## The world

`core/src/world` holds everything a part is made of:

- `topo`: one boundary representation, a coedge-based B-rep (Body, Lump, Shell, Face, Loop, Coedge, Edge, Vertex) in arenas with typed ids, with NURBS geometry only (no analytic surface types), one scalar tolerance per vertex, edge and face, and persistent names.
- Datums (planes, axes, frames), profiles (a sketch's solved curves and the regions they bound), annotations (dimensions, tolerances, notes), looks (materials and colours), threads, views and visibility.
- Instances of other parts, joints and couplings (assemblies).

## Operations, builder, outputs

An operation implements the `Operation` trait (`core/src/operation.rs`): an id, `Args`, an optional `EditState` (the tool in use while editing, not saved), and one of

- `make(&self, builder) -> Made`: for operations that only create things. `Made` lists what is made: bodies (by name), datums, profiles, annotations, looks, instances, joints, what to keep in the cache, and which bodies are removed. The builder puts it into the world. The operation changes nothing itself.
- `apply(&self, builder)`: for operations that change the world in place.

The `Builder` trait is what an operation sees: the world, the cache, the part's parameters, the other parts (`build(path)`), and the earlier steps. Reads go through the builder, which can note them in a `Trace` (what the step read and which bodies it wrote). Two implementations: the engine's (in `cad-part`), and `Standalone`, which applies operations to a lone world for tests.

**References** (`core/src/reference.rs`) point into the world by name, never by arena id, and are typed by what they must resolve to (`Reference<DatumPlane>` accepts a datum plane, a frame's plane or a planar face). Faces and edges are named after the operation that made them (`extrude2/side/e7`). Renaming a step follows through its references.

**Naming fallback** (`cad/cad-part/src/naming.rs`): when the history upstream changes so that a named piece comes about another way, the part's cache remembers where each referred piece was, and the nearest piece of the same kind is taken in its place if it is clearly the one. Otherwise the step fails.

## The cache

`Cache` (`core/src/builder.rs`) is a map from names (`sketch1/p3`) to numbers: what an operation solved the last time (a sketch's points, an assembly's poses). It is **an input of the build**, saved in the part file: constraints often have several solutions, and the one a build finds depends on where it starts. Once it holds a solution, that solution stays the starting point. It is not a disposable memo table.

## The engine

`cad/cad-part/src/engine.rs` builds parts and no more than it must.

- Everything is keyed by what it is made from: the steps so far (their arguments), the parameters, the cache entries the steps start from, the placed parts (recursively) and the files read. These are hashed into a key, step after step, and the world after every step is kept under its key. A build finds the longest kept prefix of its steps and applies only what comes after.
- A step is skipped even inside that when what it read (from the `Trace`) is unchanged; `effect.rs` keeps what a step did so it need not be applied again.
- Parts that a part places are built side by side on a work-stealing `Pool`. A `Cancel` stops a build at the next checkpoint a step passes; a step that panics is a step that failed.
- Poses of an assembly (worked out from joints) are computed after the last step on a copy, so dragging a part does not invalidate what is drawn.

## The registry

`core/src/registry.rs`: each operations crate has `register(&mut Registry)`, which says, for each operation, its kind (the name steps have in the file), the tools that add it (label, icon, hotkey, ribbon tab and place) and the icons it draws. `cad/cad-part/src/any_operation.rs` calls every crate's `register` once and installs the result. Nothing else lists the operations: a part holds its steps as `AnyOperation`, which is any registered operation. Adding an operation means writing it in a crate and registering it there (and adding the crate to that list if it is new).

## From click to picture

In the app, each frame `cad-ui` derives a `View` from the `Model`; `hosts/app` draws it with egui. What the user does becomes `Message`s that are applied to the model after the frame. The model edits the part's program, asks the workspace to build it (`Workspace::build`, via the engine), and `cad-rasterize` turns the world into buffers that the host draws. Previews of a step being edited are built the same way on a copy.

## In the browser

`hosts/app` compiled for `wasm32-unknown-unknown` runs in a page (eframe's web runner, WebGL 2 through the same `glow` renderer). What differs is said in a few places, not spread through the code: `workpiece_core::files` reads and writes the files the user names, on disk or (in a browser) in memory, which is what the kernel's own reads (a placed or imported STEP file) go through too; `workpiece_core::HAS_THREADS` says whether work can go to threads (the engine's pool, the drawing's views, the previews, the path tracer) or is done on the one thread there is; and `hosts/app/src/web.rs` has the page: opening by `?open=`, picking local files, downloads, the start screen with the examples, the settings in local storage. Without threads a build runs as it is asked for, on the page's thread. The standard library for this target is built with `panic=abort`, so a step that panics ends the page (it says so) instead of failing as a step.

## The command line and Python

`hosts/cli` reads a JSON recipe (steps compactly, sketches by named points with constraints), builds the part through the same workspace, and prints a report: failed steps with reasons, each sketch's degrees of freedom, volumes after every step, and the named faces and edges. `python/workpiece/` (the `workpiece` package) writes recipes from Python and reads the report back.

## Where to start

| To | Read |
|---|---|
| add a modelling operation | `ops/basic-shapes` (small), `core/src/operation.rs`, then register it in the crate's `lib.rs` |
| change the geometry | `lib/shape`, `lib/boolean` (`lib.rs` explains the method) |
| understand a sketch | `ops/sketch`, `lib/solver` |
| change what the app shows | `cad/cad-ui` (state and view), `hosts/app/src/ui` (drawing) |
| see how builds are reused | `cad/cad-part/src/engine.rs`, `effect.rs` |
