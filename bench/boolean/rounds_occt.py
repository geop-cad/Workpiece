"""Rounds the edges shapecraft rounded with Open CASCADE, and compares what came out.

    boolean-bench fillet-corpus DIR/shapecraft.tsv DIR/*.stp DIR/*.step
    python3 rounds_occt.py DIR

DIR holds the STEP files and shapecraft.tsv (a line an edge: where it is, the radius, the volume and
area before and after). For each line the file is read, the edge nearest the point shapecraft named
is found, and it is rounded with the same radius (BRepFilletAPI_MakeFillet), each in a process
of its own. What the round changed, in volume and area, is held against what shapecraft's changed;
DIR/compared.tsv has it edge by edge. Needs cadquery-ocp (found on sys.path, or in a "venv"
folder beside this file). Run it where it cannot do harm: it executes a third-party kernel on
files from the internet (README.md, "Rounds on public parts").
"""
import csv
import math
import os
import pickle
import sys
import time

# (Run with -I: the bindings are found where they were installed, beside this file.)
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "venv", "lib", "python3.11", "site-packages"))

from OCP.BRepBndLib import BRepBndLib
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeVertex
from OCP.BRepCheck import BRepCheck_Analyzer
from OCP.BRepExtrema import BRepExtrema_DistShapeShape
from OCP.BRepFilletAPI import BRepFilletAPI_MakeFillet
from OCP.BRepGProp import BRepGProp
from OCP.Bnd import Bnd_Box
from OCP.GProp import GProp_GProps
from OCP.STEPControl import STEPControl_Reader
from OCP.TopAbs import TopAbs_EDGE, TopAbs_SOLID
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS
from OCP.gp import gp_Pnt

HERE = os.path.dirname(os.path.abspath(__file__))
DIR = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "corpus")


def measures(shape):
    props = GProp_GProps()
    BRepGProp.VolumeProperties_s(shape, props)
    volume = props.Mass()
    props = GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, props)
    return volume, props.Mass()


def all_solids(path):
    reader = STEPControl_Reader()
    if reader.ReadFile(path) != 1:
        return []
    reader.TransferRoots()
    shape = reader.OneShape()
    out = []
    solids = TopExp_Explorer(shape, TopAbs_SOLID)
    while solids.More():
        out.append(TopoDS.Solid(solids.Current()))
        solids.Next()
    return out


def find(solids, row):
    """The solid and edge shapecraft means, and the scale of OCCT's lengths to shapecraft's (OCCT reads
    in millimetres whatever the file's unit is)."""
    for scale in (1.0, 25.4, 1000.0, 10.0, 1 / 25.4, 0.001):
        point = [float(row[k]) * scale for k in "xyz"]
        within = 1e-4 * float(row["diagonal"]) * scale
        for solid in solids:
            edge = edge_at(solid, point, within)
            if edge is not None:
                return solid, edge, scale
    return None, None, 1.0


def diagonal(shape):
    box = Bnd_Box()
    BRepBndLib.Add_s(shape, box)
    lo, hi = box.CornerMin(), box.CornerMax()
    return math.dist((lo.X(), lo.Y(), lo.Z()), (hi.X(), hi.Y(), hi.Z()))


def edge_at(solid, point, within):
    vertex = BRepBuilderAPI_MakeVertex(gp_Pnt(*point)).Vertex()
    best = (within, None)
    edges = TopExp_Explorer(solid, TopAbs_EDGE)
    while edges.More():
        edge = TopoDS.Edge(edges.Current())
        far = BRepExtrema_DistShapeShape(vertex, edge)
        if far.IsDone() and far.Value() < best[0]:
            best = (far.Value(), edge)
        edges.Next()
    return best[1]


def one(solids, row, result):
    """Rounds the edge of one line; what came of it goes into `result`."""
    solid, edge, scale = find(solids, row)
    if edge is None:
        result["occt"] = "edge not found"
        return
    radius = float(row["radius"]) * scale
    before = measures(solid)
    started = time.time()
    try:
        fillet = BRepFilletAPI_MakeFillet(solid)
        fillet.Add(radius, edge)
        # (OCCT rounds on along every edge that runs on smoothly from the one named.)
        result["chain"] = fillet.NbEdges(1)
        fillet.Build()
        if not fillet.IsDone():
            raise RuntimeError("not done")
        made = fillet.Shape()
        if not BRepCheck_Analyzer(made).IsValid():
            raise RuntimeError("invalid result")
        after = measures(made)
        result["occt"] = "ok"
        result["occt_dv"] = (after[0] - before[0]) / scale**3
        result["occt_da"] = (after[1] - before[1]) / scale**2
    except Exception as error:  # noqa: BLE001 (whatever the kernel throws is a refusal)
        result["occt"] = "refused: " + str(error).splitlines()[0][:80]
    result["occt_ms"] = round((time.time() - started) * 1e3, 1)



def main():
    rows = list(csv.DictReader(open(os.path.join(DIR, "shapecraft.tsv")), delimiter="\t"))
    solids = {}
    out = []
    for row in rows:
        name = row["file"]
        if name not in solids:
            solids[name] = all_solids(os.path.join(DIR, name))
        result = dict(row, occt="no solid", occt_dv="", occt_da="", occt_ms="", chain="")
        out.append(result)
        if not solids[name]:
            continue
        # (In a process of its own: a kernel that crashes on an edge takes only that edge down.)
        read, write = os.pipe()
        child = os.fork()
        if child == 0:
            os.close(read)
            try:
                one(solids[name], row, result)
            except BaseException as error:  # noqa: BLE001
                result["occt"] = "refused: " + str(error).splitlines()[0][:80]
            with os.fdopen(write, "wb") as pipe:
                pickle.dump(result, pipe)
            os._exit(0)
        os.close(write)
        with os.fdopen(read, "rb") as pipe:
            data = pipe.read()
        os.waitpid(child, 0)
        if data:
            result.update(pickle.loads(data))
        else:
            result["occt"] = "crashed"

    # The comparison.
    both = agree = only_shapecraft = only_occt = neither = chained = unmatched = 0
    worst = []
    for r in out:
        g_ok, o_ok = r["status"] == "ok", r["occt"] == "ok"
        if r["occt"] in ("edge not found", "no solid"):
            unmatched += 1
        elif r["chain"] not in ("", 1):
            chained += 1
        elif g_ok and o_ok:
            both += 1
            g_dv = float(r["volume_after"]) - float(r["volume"])
            g_da = float(r["area_after"]) - float(r["area"])
            dv = abs(g_dv - r["occt_dv"]) / max(abs(r["occt_dv"]), 1e-300)
            da = abs(g_da - r["occt_da"]) / max(abs(r["occt_da"]), 1e-300)
            r["dv_off"], r["da_off"] = dv, da
            if dv <= 1e-3 and da <= 1e-3:
                agree += 1
            else:
                worst.append((max(dv, da), r))
        elif g_ok:
            only_shapecraft += 1
        elif o_ok:
            only_occt += 1
        else:
            neither += 1
    with open(os.path.join(DIR, "compared.tsv"), "w") as f:
        keys = ["file", "edge", "radius", "status", "ms", "occt", "occt_ms", "chain", "dv_off", "da_off", "why"]
        f.write("\t".join(keys) + "\n")
        for r in out:
            f.write("\t".join(str(r.get(k, "")) for k in keys) + "\n")
    g_total = sum(r["status"] == "ok" for r in out)
    o_total = sum(r["occt"] == "ok" for r in out)
    print(f"edges {len(out)}  shapecraft ok {g_total}  occt ok {o_total}")
    print(f"not matched {unmatched}  occt rounded a chain of edges {chained}")
    single = both + only_shapecraft + only_occt + neither
    print(f"single edges {single}: both ok {both} (agree within 0.1% {agree})  only shapecraft {only_shapecraft}  only occt {only_occt}  neither {neither}")
    ms = lambda key: sorted(float(r[key]) for r in out if r.get(key) not in ("", None))  # noqa: E731
    g_ms, o_ms = ms("ms"), ms("occt_ms")
    print(f"median ms: shapecraft {g_ms[len(g_ms) // 2]:.0f}  occt {o_ms[len(o_ms) // 2]:.0f}")
    for off, r in sorted(worst, key=lambda w: -w[0])[:10]:
        print(f"  differ {off:.3%}: {r['file']} edge {r['edge']}")
    for r in out:
        if r["status"] != "ok" and r["occt"] == "ok" and r["chain"] == 1:
            print(f"  shapecraft refused: {r['file']} edge {r['edge']}: {r['why'][:150]}")
    for r in out:
        if r["occt"] not in ("ok", "edge not found") and r["status"] == "ok" and r["chain"] in ("", 1):
            print(f"  occt: {r['file']} edge {r['edge']}: {r['occt']}")


if __name__ == "__main__":
    sys.exit(main())
