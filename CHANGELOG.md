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

### Changed
- Saving a part saves everything it needs to be opened again: each part it places, in turn, that is changed or has no file is written too, a part after those it places; nothing is written over a file that is not the part's own or that changed on disk since it was read, and a failure says what was written and what not. A new part made in an assembly (Assembly → New part) is written to its file at once, beside the assembly, and the assembly must have a file first. Files are written whole or not at all.
- A part file names the parts it places relative to its own folder; the app and the command line read them so, whatever the working directory.
- What is not saved shows: a dot on the title and on each placed part in the timeline that is not saved as it is. Closing a part, opening another over it and closing the window ask first (Save all, Discard, Cancel) when work would be lost; in the browser, leaving the page asks too. A placed part opens on its own from its row in the timeline.
- A part opened from the command line counts as saved as it was read.
