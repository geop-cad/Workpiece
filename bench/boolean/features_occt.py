"""The `features` cases of boolean-bench (src/features.rs), done with Open CASCADE (cadquery-ocp) and
graded the same way: each has an answer in closed form, and passes within a relative 1e-6.

Run it where OCP can be imported: python3 features_occt.py
"""
import math
import os
import sys
import tempfile
import time
import traceback

from OCP.BRepAlgoAPI import BRepAlgoAPI_Common, BRepAlgoAPI_Cut, BRepAlgoAPI_Fuse, BRepAlgoAPI_Section
from OCP.BRepBuilderAPI import (
    BRepBuilderAPI_MakeEdge,
    BRepBuilderAPI_MakeFace,
    BRepBuilderAPI_MakePolygon,
    BRepBuilderAPI_MakeSolid,
    BRepBuilderAPI_Sewing,
)
from OCP.BRepFilletAPI import BRepFilletAPI_MakeFillet
from OCP.BRepGProp import BRepGProp
from OCP.BRepLib import BRepLib
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeCylinder, BRepPrimAPI_MakePrism, BRepPrimAPI_MakeSphere
from OCP.Geom import Geom_CylindricalSurface
from OCP.Geom2d import Geom2d_Line
from OCP.GeomAPI import GeomAPI_Interpolate
from OCP.GProp import GProp_GProps
from OCP.gp import gp_Ax2, gp_Ax3, gp_Dir, gp_Dir2d, gp_Pnt, gp_Pnt2d, gp_Vec
from OCP.Interface import Interface_Static
from OCP.STEPControl import STEPControl_AsIs, STEPControl_Reader, STEPControl_Writer
from OCP.collections import HArray1_gp_Pnt as TColgp_HArray1OfPnt
from OCP.collections import (
    IndexedDataMap_TopoDS_Shape_List_TopoDS_Shape_TopTools_ShapeMapHasher as TopTools_IndexedDataMapOfShapeListOfShape,
)
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE
from OCP.TopExp import TopExp, TopExp_Explorer
from OCP.TopoDS import TopoDS

PI, TAU = math.pi, 2 * math.pi


def volume(shape):
    props = GProp_GProps()
    BRepGProp.VolumeProperties_s(shape, props)
    return props.Mass()


def area(shape):
    props = GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, props)
    return props.Mass()


def length(shape):
    props = GProp_GProps()
    BRepGProp.LinearProperties_s(shape, props)
    return props.Mass()


def faces(shape):
    out, it = [], TopExp_Explorer(shape, TopAbs_FACE)
    while it.More():
        out.append(TopoDS.Face(it.Current()))
        it.Next()
    return out


def middle(face):
    props = GProp_GProps()
    BRepGProp.SurfaceProperties_s(face, props)
    c = props.CentreOfMass()
    return (c.X(), c.Y(), c.Z())


def towards(shape, way):
    return max(faces(shape), key=lambda f: sum(a * b for a, b in zip(middle(f), way)))


def largest(shape):
    return max(faces(shape), key=area)


def hexagon(flats):
    return math.sqrt(3) / 2 * flats * flats


def hex_prism(flats, z0, z1):
    r = flats / math.sqrt(3)
    poly = BRepBuilderAPI_MakePolygon()
    for k in range(6):
        a = TAU * k / 6
        poly.Add(gp_Pnt(r * math.cos(a), r * math.sin(a), z0))
    poly.Close()
    face = BRepBuilderAPI_MakeFace(poly.Wire()).Face()
    return BRepPrimAPI_MakePrism(face, gp_Vec(0, 0, z1 - z0)).Shape()


def cylinder(r, z0, z1, x=0.0, y=0.0):
    return BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(x, y, z0), gp_Dir(0, 0, 1)), r, z1 - z0).Shape()


def box(lo, hi):
    return BRepPrimAPI_MakeBox(gp_Pnt(*lo), gp_Pnt(*hi)).Shape()


def ring(bore, outside, height):
    return BRepAlgoAPI_Cut(cylinder(outside / 2, 0, height), cylinder(bore / 2, -height, 2 * height)).Shape()


def section(a, b):
    made = BRepAlgoAPI_Section(a, b)
    made.Build()
    return made.Shape()


def nut_m6():
    body = BRepAlgoAPI_Cut(hex_prism(10, 0, 5.2), cylinder(3, -5.2, 10.4)).Shape()
    return volume(body), (hexagon(10) - PI * 9) * 5.2


def screw(flats, head, d, long):
    return BRepAlgoAPI_Fuse(hex_prism(flats, 0, head), cylinder(d / 2, -long, 0)).Shape()


def screw_m8x30():
    return volume(screw(13, 5.3, 8, 30)), hexagon(13) * 5.3 + PI * 16 * 30


def washer_m10():
    return volume(ring(10.5, 20, 2)), PI * (100 - 5.25**2) * 2


def bearing_6204():
    return volume(ring(20, 47, 14)), PI * (23.5**2 - 100) * 14


def screw_m16_area():
    return area(screw(24, 10, 16, 60)), 2 * hexagon(24) + 6 * 24 / math.sqrt(3) * 10 + TAU * 8 * 60


def cylinder_crosses_plane():
    side = largest(cylinder(2, -5, 5))
    top = towards(box((-5, -5, -1), (5, 5, 1)), (0, 0, 1))
    return length(section(side, top)), TAU * 2


def ball_crosses_plane():
    ball = BRepPrimAPI_MakeSphere(gp_Pnt(0, 0, 0), 5).Shape()
    top = towards(box((-10, -10, 1), (10, 10, 3)), (0, 0, 1))
    return length(section(largest(ball), top)), TAU * 4


def block_sides_cross():
    block = box((-2, -3, -4), (2, 3, 4))
    return length(section(towards(block, (0, 0, 1)), towards(block, (0, -1, 0)))), 4.0


def cylinder_side_trimmed():
    side = largest(cylinder(1.5, 0, 4))
    behind = box((-10, -10, -10), (0, 10, 10))
    return area(BRepAlgoAPI_Common(side, behind).Shape()), PI * 1.5 * 4


def ball_skin_trimmed():
    ball = BRepPrimAPI_MakeSphere(gp_Pnt(0, 0, 0), 5).Shape()
    below = box((-10, -10, -10), (10, 10, 3))
    return area(BRepAlgoAPI_Common(largest(ball), below).Shape()), 4 * PI * 25 - TAU * 5 * 2


def sheet_edge_rounded():
    block = box((-2, -2, -1), (2, 2, 1))
    sew = BRepBuilderAPI_Sewing(1e-6)
    sew.Add(towards(block, (0, 0, 1)))
    sew.Add(towards(block, (0, -1, 0)))
    sew.Perform()
    sheet = sew.SewedShape()
    beside = TopTools_IndexedDataMapOfShapeListOfShape()
    TopExp.MapShapesAndAncestors_s(sheet, TopAbs_EDGE, TopAbs_FACE, beside)
    fillet = BRepFilletAPI_MakeFillet(sheet)
    r = 0.5
    for k in range(1, beside.Extent() + 1):
        if beside.FindFromIndex(k).Extent() == 2:
            fillet.Add(r, TopoDS.Edge(beside.FindKey(k)))
    fillet.Build()
    return area(fillet.Shape()), (4 - r) * 4 + (2 - r) * 4 + PI * r / 2 * 4


def mesh_tetrahedron():
    p = [(0, 0, 0), (3, 0, 0), (0, 3, 0), (0, 0, 3)]
    sew = BRepBuilderAPI_Sewing(1e-9)
    for tri in [(0, 2, 1), (0, 1, 3), (1, 2, 3), (0, 3, 2)]:
        poly = BRepBuilderAPI_MakePolygon()
        for k in tri:
            poly.Add(gp_Pnt(*p[k]))
        poly.Close()
        sew.Add(BRepBuilderAPI_MakeFace(poly.Wire()).Face())
    sew.Perform()
    solid = BRepBuilderAPI_MakeSolid(TopoDS.Shell(sew.SewedShape())).Solid()
    return abs(volume(solid)), 4.5


def step_in_inches():
    Interface_Static.SetCVal_s("write.step.unit", "INCH")
    path = os.path.join(tempfile.mkdtemp(dir=os.path.dirname(os.path.abspath(__file__))), "inch.step")
    writer = STEPControl_Writer()
    writer.Transfer(box((0, 0, 0), (1, 2, 3)), STEPControl_AsIs)
    writer.Write(path)
    Interface_Static.SetCVal_s("write.step.unit", "MM")
    reader = STEPControl_Reader()
    reader.ReadFile(path)
    reader.TransferRoots()
    # (The writer scales its numbers to inches: read back in millimetres, the box is as it was.
    # What is asked is that the file's unit is honoured, so the same box is the answer.)
    return volume(reader.OneShape()), 6.0


def spline_through_points():
    pts = [(0, 0, 0), (1, 2, 0.5), (3, 2.5, 2), (4, 0, 3)]
    arr = TColgp_HArray1OfPnt(1, len(pts))
    for k, p in enumerate(pts):
        arr.SetValue(k + 1, gp_Pnt(*p))
    made = GeomAPI_Interpolate(arr, False, 1e-9)
    made.Perform()
    curve = made.Curve()
    edge = BRepBuilderAPI_MakeEdge(curve).Edge()
    from OCP.BRepExtrema import BRepExtrema_DistShapeShape
    from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeVertex

    off = 0.0
    for p in pts:
        d = BRepExtrema_DistShapeShape(BRepBuilderAPI_MakeVertex(gp_Pnt(*p)).Vertex(), edge)
        off += d.Value()
    return 1.0 + off, 1.0


def helix_length():
    r, pitch, turns = 2.0, 0.5, 3.0
    surface = Geom_CylindricalSurface(gp_Ax3(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), r)
    line = Geom2d_Line(gp_Pnt2d(0, 0), gp_Dir2d(TAU, pitch))
    edge = BRepBuilderAPI_MakeEdge(line, surface, 0.0, turns * math.hypot(TAU, pitch)).Edge()
    BRepLib.BuildCurves3d_s(edge)
    return length(edge), turns * math.hypot(TAU * r, pitch)


CASES = [
    ("nut-m6", nut_m6),
    ("screw-m8x30", screw_m8x30),
    ("washer-m10", washer_m10),
    ("bearing-6204", bearing_6204),
    ("screw-m16-area", screw_m16_area),
    ("cylinder-crosses-plane", cylinder_crosses_plane),
    ("ball-crosses-plane", ball_crosses_plane),
    ("block-sides-cross", block_sides_cross),
    ("cylinder-side-trimmed", cylinder_side_trimmed),
    ("ball-skin-trimmed", ball_skin_trimmed),
    ("sheet-edge-rounded", sheet_edge_rounded),
    ("mesh-tetrahedron", mesh_tetrahedron),
    ("step-in-inches", step_in_inches),
    ("spline-through-points", spline_through_points),
    ("helix-length", helix_length),
]

passed = 0
for name, case in CASES:
    started = time.time()
    try:
        got, wanted = case()
        off = abs((got - wanted) / wanted)
        grade = "pass" if off <= 1e-6 else "wrong"
        note = "" if grade == "pass" else f"{got} is not {wanted} (off by {off:.2e})"
    except Exception:  # noqa: BLE001
        grade, note = "crashed", traceback.format_exc().strip().splitlines()[-1]
    passed += grade == "pass"
    print(f"{grade:8} {name:26} {1000 * (time.time() - started):8.1f} ms  {note}")
print(f"{passed} of {len(CASES)} pass")
sys.exit(0)
