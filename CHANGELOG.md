# Changelog

All notable changes to Workpiece are written here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). There are no releases yet, so there is no versioning policy to state; the part file format and the APIs are not stable.

## [Unreleased]

### Added
- The first release (0.1.0) contains: the NURBS B-rep kernel with booleans, fillets and chamfers, sweeps and lofts, shells and drafts; the sketcher with its constraint solver; the Modeling, Surfacing, Assembly, Sheet metal, Annotate and Properties tabs of the desktop app; ISO drawings with PDF and DXF export; STEP, IGES, STL and 3MF exchange; the `workpiece` command line; and the Python layer.
- Release builds for Linux x86_64, macOS (arm64 and x86_64, a `.app` in a `.dmg`) and Windows x86_64 (a zip), each with the licences and a third-party notice. They are not signed.
- The Python package `workpiece` (import name `workpiece`): wheels carry the command line and the desktop app, so `pip install workpiece` and `python -m workpiece part.wpc` need no Rust. Helpers are at `python -m workpiece.tools`.
- `--version` and `--help` on the command line and the app; the Windows app has an icon and a version resource and opens no console window.
- The first public preparation of the repository: licences (MIT or Apache-2.0), README, contribution and security documents, continuous integration, the documentation book and the website.
