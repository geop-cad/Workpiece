# workpiece

Workpiece is a parametric CAD kernel (B-reps on NURBS), a desktop application and a command
line, with a Python layer for writing parts as scripts. This package brings all of it: the
wheel carries the `workpiece` command line and the `workpiece-app` desktop application for
your platform, so there is nothing else to install.

```
pip install workpiece            # the import name is `workpiece`
pip install "workpiece[images]"  # with Pillow, for workpiece.crop and workpiece.tile
```

```python
from workpiece import Part

p = Part()
s = p.sketch("XY")
r = s.rect("a", 0, 0, 60, 40)
s.coincident(r.points[0], s.origin)
s.distance_u(r.points[0], r.points[1], 60)
s.distance_v(r.points[1], r.points[2], 40)
p.extrude(s, 10)
p.build("plate.wpc").show()
```

| Command | What it does |
|---|---|
| `python -m workpiece [part.wpc]` or `workpiece-app` | the desktop application, with a part opened |
| `workpiece ...` or `python -m workpiece.cli ...` | the command line (`kinds`, `make`, `report`) |
| `python -m workpiece.tools help [topic]` or `workpiece-tools` | the layer's README by topic; `sheet`, `crop`, `tile` |

PDF sheets are turned into pictures with poppler's `pdftoppm`, which is not a Python package
(`apt install poppler-utils`, `brew install poppler`); without it a build leaves the pictures out.

Documentation and source: https://github.com/geop-cad/Workpiece. Licensed under MIT or Apache-2.0.
