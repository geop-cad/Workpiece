# Bench vise (assembly)

A cast body with a fixed jaw, a moving jaw and an M12 screw: the jaw slides on the body (a slider joint), the screw turns in the jaw's seat (a revolute joint), and a Drive step opens the vise to a width. The script builds it opened 40 (`bench_vise.workpiece`, `render.png`) and nearly shut (`bench_vise_closed.workpiece`, `render-closed.png`); the section through the screw draws the screw whole, as it is not sectioned.

Run `python3 examples/bench_vise/bench_vise.py` to rebuild `bench_vise.workpiece` (open it in Workpiece), or `python3 examples/build_all.py bench_vise` to refresh `drawing.pdf`, `drawing.svg`, `thumbnail.png` and `render.png` as well.
