# Workpiece

Workpiece is a CAD kernel and a desktop application written in Rust. A part is a list of steps that the program applies in order to build solids and surfaces on NURBS geometry; changing an early step rebuilds the ones after it. Parts can be edited in the app, scripted from Python through a command line, put together into assemblies with joints, and drawn on ISO sheets.

It is an early prototype. The part file format and the crate APIs are not stable.

- [Install and run](install.md): build from source.
- [Features](features.md): what works today, and the [reference by tab](reference.md).
- [Scripting](scripting.md): build parts from Python.
- [Architecture](architecture.md): how the crates fit together and how a part is built.
- [Benchmarks](benchmarks.md): the boolean suite and the brep-conformance adapter.

The source, the issue tracker and the contribution guide are in the [repository](https://github.com/geop-cad/Workpiece). Workpiece is licensed under MIT or Apache-2.0, at your option.
