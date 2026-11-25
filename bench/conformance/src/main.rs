//! shapecraft as an adapter for [brep-conformance](https://github.com/springcraft-ai/brep-conformance):
//! one JSON request per line on stdin, one reply per line on stdout.
//!
//! The suite's rule for an adapter is kept: every function is answered by
//! the kernel's one call for it, or it is not in `supports`. What is done
//! here besides calling the kernel is what the rule allows — giving a
//! profile's segments as the kernel's curves, a frame as its placement,
//! finding the edge or face a point names with the kernel's own "is this
//! point on it" at the suite's `10·ε`, and reading the solids of a STEP
//! file back as their union (which the suite's contract asks of an
//! adapter). Nothing is retried, repaired or decided here, and no
//! answer is worked out by the adapter itself.
//!
//! A panic of the kernel is caught and answered as a crash.

mod profile;

use std::io::{BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use glam::DMat3;
use serde_json::{Value, json};
use shapecraft_boolean::geometry::{chords, on_curve};
use shapecraft_boolean::{
    Op, Placer, Side, Solid, combine, combine_all, lies_on, measure, read, write,
};
use shapecraft_core::math::{Axis, Point3};
use shapecraft_core::world::topo::{Curve3, Topo};
use shapecraft_core::world::{Datum, DatumFrame};
use shapecraft_core::{Measure, Operation, Pose, Reference, Standalone};
use shapecraft_ops_basic_shapes::{
    AddBox, AddBoxArgs, AddCone, AddConeArgs, AddCylinder, AddCylinderArgs, AddSphere,
    AddSphereArgs, AddTorus, AddTorusArgs,
};
use shapecraft_ops_sweep::{Extrude, Revolve, RevolveAxis, SweptLoft, SweptLoftArgs};
use shapecraft_shape::roll::Rolled;
use shapecraft_shape::swept::{area_along_path, through_sections};
use shapecraft_shape::{BlendShape, By, Combine, Section, blend_chains, blend_solid};

/// The suite's `10·ε`: how near a point is to the edge or face it names.
const NAMES: f64 = 1e-6;

/// The functions shapecraft has one call for.
const SUPPORTS: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
    "torus",
    "cone",
    "extrude",
    "revolve",
    "screw",
    "loft",
    "loft_through",
    "sweep",
    "sweep_path",
    "slab",
    "union",
    "intersection",
    "difference",
    "union_all",
    "difference_all",
    "fillet",
    "fillet_variable",
    "fillet_conic",
    "fillet_g2",
    "fillet_setback",
    "fillet_setback_g2",
    "fillet_radii",
    "fillet_faces",
    "full_round",
    "chamfer",
    "chamfer_two",
    "chamfer_distances",
    "offset",
    "offset_faces",
    "delete_faces",
    "move_faces",
    "thicken",
    "shell",
    "draft",
    "write_step",
    "read_step",
    "sew",
    "interference",
    "distance",
    "project",
    "display",
    "volume",
    "area",
    "classify",
    "raycast",
    "normal",
    "curvature",
    "closest",
    "bounding_box",
    "section",
    "affine",
    "check",
    "centroid",
    "split",
    "transform",
];

/// Why a request is not answered with a result.
pub enum No {
    /// The kernel has not built this.
    Unsupported(String),
    /// The request names an edge or a face that is not there.
    Invalid(String),
    /// The kernel declined: its errors do not say more than that.
    Failed(String),
}

impl From<anyhow::Error> for No {
    fn from(e: anyhow::Error) -> Self {
        No::Failed(format!("{e:#}"))
    }
}

impl From<shapecraft_boolean::Refusal> for No {
    fn from(e: shapecraft_boolean::Refusal) -> Self {
        No::Failed(e.to_string())
    }
}

impl From<shapecraft_core::world::topo::NurbsError> for No {
    fn from(e: shapecraft_core::world::topo::NurbsError) -> Self {
        No::Failed(e.to_string())
    }
}

type Answer<T> = Result<T, No>;

fn malformed(what: &str) -> No {
    No::Failed(format!("malformed request: {what}"))
}

fn number(v: &Value, key: &str) -> Answer<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| malformed(key))
}

fn point(v: &Value) -> Answer<Point3> {
    let a = v.as_array().filter(|a| a.len() == 3);
    let a = a.ok_or_else(|| malformed("a point"))?;
    let c = |k: usize| a[k].as_f64().ok_or_else(|| malformed("a coordinate"));
    Ok(Point3::new(c(0)?, c(1)?, c(2)?))
}

thread_local! {
    /// The `points` of the request being answered, read as numbers
    /// straight away (see [`Request`]).
    static POINTS: std::cell::RefCell<Option<Vec<Point3>>> = const { std::cell::RefCell::new(None) };
}

/// A request: its `points` (thousands of them, where points are placed
/// against a solid) read as numbers straight away, and all else of it
/// as it is written.
struct Request {
    rest: Value,
    points: Option<Vec<Point3>>,
}

impl<'de> serde::Deserialize<'de> for Request {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> serde::de::Visitor<'de> for Fields {
            type Value = Request;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a request: an object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Request, A::Error> {
                let mut rest = serde_json::Map::new();
                let mut points = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "points" {
                        let read: Vec<[f64; 3]> = map.next_value()?;
                        points = Some(read.into_iter().map(Point3::from_array).collect());
                    } else {
                        rest.insert(key, map.next_value()?);
                    }
                }
                Ok(Request {
                    rest: Value::Object(rest),
                    points,
                })
            }
        }
        d.deserialize_map(Fields)
    }
}

fn points(v: &Value, key: &str) -> Answer<Vec<Point3>> {
    if key == "points"
        && let Some(read) = POINTS.with(|p| p.borrow_mut().take())
    {
        return Ok(read);
    }
    let list = v.get(key).and_then(Value::as_array);
    list.ok_or_else(|| malformed(key))?
        .iter()
        .map(point)
        .collect()
}

fn out(p: Point3) -> Value {
    json!([p.x, p.y, p.z])
}

fn finite(x: f64, what: &str) -> Answer<Value> {
    if x.is_finite() {
        Ok(json!(x))
    } else {
        Err(No::Failed(format!("the {what} is not a number")))
    }
}

/// A frame of the suite.
pub struct Frame {
    /// Its origin.
    pub origin: Point3,
    /// Its axes.
    pub axes: [Point3; 3],
}

impl Frame {
    fn of(v: &Value) -> Answer<Frame> {
        let f = v.get("frame").ok_or_else(|| malformed("frame"))?;
        let at = |key: &str| point(f.get(key).ok_or_else(|| malformed(key))?);
        Ok(Frame {
            origin: at("origin")?,
            axes: [at("x")?, at("y")?, at("z")?],
        })
    }

    /// Where the point (u, v, w) of the frame is.
    pub fn place(&self, u: f64, v: f64, w: f64) -> Point3 {
        self.origin + self.axes[0] * u + self.axes[1] * v + self.axes[2] * w
    }
}

/// A world with the frame at `origin` whose x and y are as given, called
/// `frame`.
fn world(origin: Point3, x: Point3, y: Point3) -> Answer<Standalone> {
    let mut b = Standalone::new();
    let frame = DatumFrame::from_pose(Pose::from_axes(origin, x, y))
        .map_err(|e| No::Failed(e.to_string()))?;
    b.world.datums.insert("frame".into(), Datum::Frame(frame));
    Ok(b)
}

fn frame_ref() -> Option<Reference<DatumFrame>> {
    Some(Reference::datum("frame"))
}

/// The body `id` of the world, as a solid.
fn take(b: &Standalone, id: &str) -> Answer<Solid> {
    let body = b.world.topo.resolve(id);
    let body = body.ok_or_else(|| No::Failed(format!("no body {id} was made")))?;
    Ok(read(&b.world.topo, body)?)
}

/// The solids the worker holds: handle `k` is `solids[k - 1]`.
#[derive(Default)]
struct Worker {
    solids: Vec<Solid>,
    /// What each solid measures, once it was asked: a solid is not
    /// changed after it is made, and its volume, area and centre are
    /// one call of the kernel.
    measured: Vec<std::cell::OnceCell<shapecraft_boolean::Measures>>,
    /// What answers questions about each solid, once one was asked:
    /// what the kernel makes ready for them is made once.
    placers: Vec<std::cell::OnceCell<Placer<'static>>>,
}

impl Worker {
    fn keep(&mut self, solid: Solid) -> Value {
        self.solids.push(solid);
        self.measured.push(std::cell::OnceCell::new());
        self.placers.push(std::cell::OnceCell::new());
        json!(self.solids.len())
    }

    /// What answers questions about the solid `s` of `v`.
    fn placer(&self, v: &Value) -> Answer<&Placer<'static>> {
        let handle = v.get("s").ok_or_else(|| malformed("s"))?;
        let solid = self.held(handle)?;
        let k = handle.as_u64().unwrap_or(1) as usize - 1;
        Ok(self.placers[k].get_or_init(|| Placer::holding(solid.clone())))
    }

    /// What the solid `key` of `v` names measures.
    fn measures(&self, v: &Value, key: &str) -> Answer<shapecraft_boolean::Measures> {
        let handle = v.get(key).ok_or_else(|| malformed(key))?;
        let solid = self.held(handle)?;
        let k = handle.as_u64().unwrap_or(1) as usize - 1;
        Ok(*self.measured[k].get_or_init(|| measure(solid)))
    }

    fn solid(&self, v: &Value, key: &str) -> Answer<&Solid> {
        self.held(v.get(key).ok_or_else(|| malformed(key))?)
    }

    fn held(&self, handle: &Value) -> Answer<&Solid> {
        let k = handle.as_u64().ok_or_else(|| malformed("a handle"))? as usize;
        k.checked_sub(1)
            .and_then(|k| self.solids.get(k))
            .ok_or_else(|| malformed("a handle that was not issued"))
    }

    fn answer(&mut self, cmd: &str, v: &Value) -> Answer<Value> {
        match cmd {
            "hello" => Ok(json!({
                "name": "shapecraft",
                "version": env!("CARGO_PKG_VERSION"),
                "tolerance": 1e-7,
                "configuration": "",
                "protocol": "1.1.0",
                "supports": SUPPORTS,
            })),
            "reset" => {
                self.solids.clear();
                self.measured.clear();
                self.placers.clear();
                Ok(Value::Null)
            }
            "box" | "cylinder" | "sphere" | "torus" | "cone" => {
                let solid = primitive(cmd, v)?;
                Ok(self.keep(solid))
            }
            "slab" => {
                let solid = slab(v)?;
                Ok(self.keep(solid))
            }
            "extrude" | "revolve" | "screw" => {
                let solid = sketched(cmd, v)?;
                Ok(self.keep(solid))
            }
            "loft" | "loft_through" | "sweep" | "sweep_path" => {
                let solid = swept(cmd, v)?;
                Ok(self.keep(solid))
            }
            "union" | "intersection" | "difference" => {
                let op = match cmd {
                    "union" => Op::Union,
                    "intersection" => Op::Intersect,
                    _ => Op::Cut,
                };
                let made = combine(op, self.solid(v, "a")?, self.solid(v, "b")?)?.solid;
                Ok(self.keep(made))
            }
            "chamfer_distances" => {
                let s = self.solid(v, "s")?;
                let at = points(v, "edges")?;
                let distances = v.get("distances").and_then(Value::as_array);
                let distances = distances.ok_or_else(|| malformed("distances"))?;
                if distances.len() != at.len() {
                    return Err(malformed("a distance for each edge"));
                }
                let mut blends = Vec::new();
                for (p, d) in at.into_iter().zip(distances) {
                    let d = d.as_f64().ok_or_else(|| malformed("a distance"))?;
                    let shape = BlendShape::Chamfer {
                        first: d,
                        second: d,
                    };
                    blends.push((edge_at(s, p)?, shape));
                }
                let made = shapecraft_shape::blend_each(s, cmd, &blends)?;
                Ok(self.keep(made))
            }
            "fillet_radii" => {
                let s = self.solid(v, "s")?;
                let at = points(v, "edges")?;
                let radii = v.get("radii").and_then(Value::as_array);
                let radii = radii.ok_or_else(|| malformed("radii"))?;
                if radii.len() != at.len() {
                    return Err(malformed("a radius for each edge"));
                }
                let mut blends = Vec::new();
                for (p, r) in at.into_iter().zip(radii) {
                    let shape = BlendShape::Fillet {
                        by: By::Radius,
                        size: r.as_f64().ok_or_else(|| malformed("a radius"))?,
                        until: None,
                        section: Section::Circular,
                        rho: 0.5,
                    };
                    blends.push((edge_at(s, p)?, shape));
                }
                let made = shapecraft_shape::blend_each(s, cmd, &blends)?;
                Ok(self.keep(made))
            }
            "fillet_faces" => {
                let s = self.solid(v, "s")?;
                let faces = [
                    face_at(s, named(v, "face_a")?)?,
                    face_at(s, named(v, "face_b")?)?,
                ];
                let shape = BlendShape::Fillet {
                    by: By::Radius,
                    size: number(v, "radius")?,
                    until: None,
                    section: Section::Circular,
                    rho: 0.5,
                };
                let made = shapecraft_shape::blend_faces(s, cmd, faces, &shape)?;
                Ok(self.keep(made))
            }
            "full_round" => {
                let s = self.solid(v, "s")?;
                let sides = [
                    face_at(s, named(v, "side_a")?)?,
                    face_at(s, named(v, "side_b")?)?,
                ];
                let middle = face_at(s, named(v, "middle")?)?;
                let made = shapecraft_shape::full_round(s, cmd, sides, middle)?;
                Ok(self.keep(made))
            }
            "fillet" | "fillet_variable" | "fillet_conic" | "fillet_g2" | "fillet_setback"
            | "fillet_setback_g2" | "chamfer" | "chamfer_two" => {
                let solid = blended(cmd, self.solid(v, "s")?, v)?;
                Ok(self.keep(solid))
            }
            "offset" | "offset_faces" | "delete_faces" | "move_faces" | "thicken" | "shell"
            | "draft" => {
                let solid = edited(cmd, self.solid(v, "s")?, v)?;
                Ok(self.keep(solid))
            }
            "transform" => {
                let solid = moved(self.solid(v, "s")?, v)?;
                Ok(self.keep(solid))
            }
            "split" => {
                let s = self.solid(v, "s")?;
                let origin = point(v.get("origin").ok_or_else(|| malformed("origin"))?)?;
                let normal = point(v.get("normal").ok_or_else(|| malformed("normal"))?)?;
                let [front, behind] = shapecraft_shape::halves(s, origin, normal)?;
                let front = self.keep(front);
                let behind = self.keep(behind);
                Ok(json!([front, behind]))
            }
            "write_step" => {
                let s = self.solid(v, "s")?;
                let bodies = [("solid".to_owned(), s.clone())];
                let unit = shapecraft_core::Unit::Millimetre;
                Ok(json!(shapecraft_ops_step::export("solid", &bodies, unit)?))
            }
            "read_step" => {
                let text = v.get("text").and_then(Value::as_str);
                let text = text.ok_or_else(|| malformed("text"))?;
                let file = shapecraft_ops_step::import(text.as_bytes())?;
                if let Some(why) = file.refused.first() {
                    return Err(No::Failed(format!(
                        "a solid of the file was not read: {why}"
                    )));
                }
                if file.bodies.is_empty() {
                    return Err(No::Failed("the file holds no solid".into()));
                }
                // The solids of a file are one part's shape: read back as
                // their union, as the suite's contract has an adapter do
                // (a result whose pieces only touch is written as a solid
                // for each piece).
                let mut bodies = file.bodies.into_iter().map(|b| b.solid);
                let mut all = bodies.next().unwrap_or_default();
                for next in bodies {
                    all = combine(Op::Union, &all, &next)?.solid;
                }
                Ok(self.keep(all))
            }
            "sew" => {
                let text = v.get("text").and_then(Value::as_str);
                let text = text.ok_or_else(|| malformed("text"))?;
                let tolerance = number(v, "tolerance")?;
                if tolerance <= 0.0 {
                    return Err(No::Invalid("the tolerance must be positive".into()));
                }
                let sheets = shapecraft_ops_step::import_sheets(text.as_bytes())?;
                let (sewn, closed) = shapecraft_boolean::stitched(&sheets, tolerance)?;
                if !closed || sewn.is_empty() {
                    return Err(No::Failed("the faces do not close into a solid".into()));
                }
                Ok(self.keep(sewn))
            }
            "display" => {
                let tolerance = number(v, "tolerance")?;
                if tolerance <= 0.0 {
                    return Err(No::Invalid("the tolerance must be positive".into()));
                }
                let shown = shapecraft_boolean::display(self.solid(v, "s")?, tolerance);
                let line = |l: &Vec<Point3>| l.iter().copied().map(out).collect::<Vec<_>>();
                let faces: Vec<Value> = shown
                    .faces
                    .iter()
                    .map(|f| {
                        json!({
                            "positions": line(&f.positions),
                            "normals": line(&f.normals),
                            "triangles": f.triangles,
                        })
                    })
                    .collect();
                Ok(json!({
                    "faces": faces,
                    "edges": shown.edges.iter().map(line).collect::<Vec<_>>(),
                    "edge_faces": shown.edge_faces,
                    "edge_turns": shown.edge_turns,
                    "edge_split": shown.edge_split,
                    "seams": shown.seams.iter().map(line).collect::<Vec<_>>(),
                }))
            }
            "project" => {
                let tolerance = number(v, "tolerance")?;
                if tolerance <= 0.0 {
                    return Err(No::Invalid("the tolerance must be positive".into()));
                }
                let d = named(v, "direction")?;
                let drawn = shapecraft_boolean::project(self.solid(v, "s")?, d, tolerance)?;
                // The suite's axes of the view: across is d × z (d × x
                // looking along z), up is −d × across.
                let d = d.normalize();
                let across = d.cross(Point3::Z);
                let across = if across.length_squared() < 1e-12 {
                    d.cross(Point3::X)
                } else {
                    across
                };
                let across = across.normalize();
                let up = (-d).cross(across);
                let lay = |lines: &[Vec<Point3>]| -> Vec<Vec<[f64; 2]>> {
                    let flat = |p: &Point3| [p.dot(across), p.dot(up)];
                    lines.iter().map(|l| l.iter().map(flat).collect()).collect()
                };
                Ok(json!({"visible": lay(&drawn.visible), "hidden": lay(&drawn.hidden)}))
            }
            "interference" => {
                let met =
                    shapecraft_boolean::interference(self.solid(v, "a")?, self.solid(v, "b")?)?;
                Ok(json!({
                    "volume": finite(met.volume, "volume")?,
                    "gap": finite(met.gap, "gap")?,
                }))
            }
            "distance" => {
                let apart = shapecraft_boolean::distance(self.solid(v, "a")?, self.solid(v, "b")?)?;
                finite(apart, "distance")
            }
            "volume" => finite(self.measures(v, "s")?.volume, "volume"),
            "area" => finite(self.measures(v, "s")?.area, "area"),
            "centroid" => {
                let s = self.solid(v, "s")?;
                if s.is_empty() {
                    return Ok(Value::Null);
                }
                let c = self.measures(v, "s")?.centre;
                if !c.is_finite() {
                    return Err(No::Failed("the centre is not a point".into()));
                }
                Ok(out(c))
            }
            "classify" => {
                let placer = self.placer(v)?;
                let points = points(v, "points")?;
                let mut found = Vec::new();
                // The suite's ε: what is within it of a face is on it.
                for (p, side) in points.iter().zip(placer.sides(&points, 1e-7)) {
                    found.push(match side {
                        Some(Side::In) => "in",
                        Some(Side::On) => "on",
                        Some(Side::Out) => "out",
                        None => return Err(No::Failed(format!("{p} could not be placed"))),
                    });
                }
                Ok(json!(found))
            }
            "raycast" => {
                let placer = self.placer(v)?;
                let cast = |origin: Point3, direction: Point3| {
                    let met = placer.meets(origin, direction);
                    met.ok_or_else(|| No::Failed(format!("the ray from {origin} met nothing sure")))
                };
                if let Some(rays) = v.get("rays").and_then(Value::as_array) {
                    let mut all = Vec::new();
                    for ray in rays {
                        let ends = ray.as_array().filter(|r| r.len() == 2);
                        let ends = ends.ok_or_else(|| malformed("rays"))?;
                        all.push(cast(point(&ends[0])?, point(&ends[1])?)?);
                    }
                    return Ok(json!(all));
                }
                Ok(json!(cast(named(v, "origin")?, named(v, "direction")?)?))
            }
            "normal" => {
                let placer = self.placer(v)?;
                let found: Vec<Value> = points(v, "points")?
                    .into_iter()
                    .map(|p| placer.normal(p, NAMES).map_or(Value::Null, out))
                    .collect();
                Ok(json!(found))
            }
            "bounding_box" => {
                let found = self.placer(v)?.bounds();
                let (lo, hi) =
                    found.ok_or_else(|| No::Failed("an empty solid has no box".into()))?;
                Ok(json!([out(lo), out(hi)]))
            }
            "section" => {
                let s = self.solid(v, "s")?;
                let tolerance = number(v, "tolerance")?;
                let (origin, normal) = (named(v, "origin")?, named(v, "normal")?);
                // Followed over the faces directly where that is sure;
                // else as what cutting the solid at the plane leaves.
                if let Some(lines) =
                    shapecraft_boolean::plane_section(s, origin, normal, tolerance / 2.0)
                {
                    let lines: Vec<Vec<Value>> = lines
                        .into_iter()
                        .map(|line| line.into_iter().map(out).collect())
                        .collect();
                    return Ok(json!(lines));
                }
                let cut = shapecraft_shape::section(s, origin, normal)?;
                let pieces: Vec<Vec<Value>> = cut
                    .iter()
                    .map(|(curve, range)| {
                        let along = chords(curve, *range, tolerance);
                        along.into_iter().map(out).collect()
                    })
                    .collect();
                Ok(json!(pieces))
            }
            "closest" => {
                let placer = self.placer(v)?;
                let mut found = Vec::new();
                for p in points(v, "points")? {
                    let near = placer.nearest(p, 1e-7);
                    let near = near.ok_or_else(|| No::Failed(format!("nothing nearest {p}")))?;
                    found.push(out(near));
                }
                Ok(json!(found))
            }
            "curvature" => {
                let placer = self.placer(v)?;
                let found: Vec<Value> = points(v, "points")?
                    .into_iter()
                    .map(|p| placer.curvature(p, NAMES).map_or(Value::Null, |k| json!(k)))
                    .collect();
                Ok(json!(found))
            }
            "affine" => {
                let s = self.solid(v, "s")?;
                let (m, t) = matrix(v)?;
                // (No length is stretched by more than the matrix's norm.)
                let most = m.to_cols_array().iter().map(|x| x * x).sum::<f64>().sqrt();
                let made = s.sheared(&|p| m * p + t, m.determinant() < 0.0, most)?;
                Ok(self.keep(made))
            }
            "union_all" | "difference_all" => {
                let list = |key: &str| -> Answer<Vec<&Solid>> {
                    let all = v.get(key).and_then(Value::as_array);
                    all.ok_or_else(|| malformed(key))?
                        .iter()
                        .map(|h| self.held(h))
                        .collect()
                };
                let made = if cmd == "union_all" {
                    let all = list("solids")?;
                    let (first, rest) = all.split_first().ok_or_else(|| malformed("solids"))?;
                    combine_all(Op::Union, first, rest)?
                } else {
                    combine_all(Op::Cut, self.solid(v, "target")?, &list("tools")?)?
                };
                Ok(self.keep(made))
            }
            "check" => Ok(json!(faults(self.solid(v, "s")?))),
            other => Err(No::Unsupported(format!(
                "shapecraft has no call for {other}"
            ))),
        }
    }
}

/// What the kernel's checker finds wrong with `solid`: it is written to
/// a topology of its own and validated there.
fn faults(solid: &Solid) -> Vec<String> {
    if solid.is_empty() {
        return Vec::new();
    }
    let mut topo = Topo::new();
    if let Err(e) = write(&mut topo, solid, "solid") {
        return vec![format!("not a solid: {e}")];
    }
    match topo.validate() {
        Ok(()) => Vec::new(),
        Err(all) => all.iter().map(|e| e.to_string()).collect(),
    }
}

fn primitive(cmd: &str, v: &Value) -> Answer<Solid> {
    let f = Frame::of(v)?;
    let id = cmd.to_owned();
    let m = |key: &str| number(v, key).map(Measure::new);
    let frame = frame_ref();
    let at = |origin: Point3| world(origin, f.axes[0], f.axes[1]);
    let b = match cmd {
        "box" => {
            let size = v.get("size").and_then(Value::as_array);
            let size = size
                .filter(|s| s.len() == 3)
                .ok_or_else(|| malformed("size"))?;
            let side = |k: usize| size[k].as_f64().ok_or_else(|| malformed("size"));
            let size = [side(0)?, side(1)?, side(2)?];
            // The kernel's box is around its frame: the frame goes to
            // the middle of the box asked for.
            let mut b = at(f.place(size[0] / 2.0, size[1] / 2.0, size[2] / 2.0))?;
            let args = AddBoxArgs {
                frame,
                size: size.map(Measure::new),
            };
            AddBox { id, args }.apply(&mut b)?;
            b
        }
        "cylinder" => {
            let mut b = at(f.origin)?;
            let args = AddCylinderArgs {
                frame,
                radius: m("radius")?,
                height: m("height")?,
            };
            AddCylinder { id, args }.apply(&mut b)?;
            b
        }
        "sphere" => {
            let mut b = at(f.origin)?;
            let args = AddSphereArgs {
                frame,
                radius: m("radius")?,
            };
            AddSphere { id, args }.apply(&mut b)?;
            b
        }
        "torus" => {
            let mut b = at(f.origin)?;
            let args = AddTorusArgs {
                frame,
                radius: m("major")?,
                tube: m("minor")?,
            };
            AddTorus { id, args }.apply(&mut b)?;
            b
        }
        _ => {
            let mut b = at(f.origin)?;
            let args = AddConeArgs {
                frame,
                radius: m("bottom")?,
                top: m("top")?,
                height: m("height")?,
            };
            AddCone { id, args }.apply(&mut b)?;
            b
        }
    };
    take(&b, cmd)
}

/// The block under a height field: the field as the kernel's surface
/// (its control points over the knots' Greville abscissae, as the suite
/// defines it), and what is under that down to the frame's plane.
fn slab(v: &Value) -> Answer<Solid> {
    let f = Frame::of(v)?;
    let numbers = |value: &Value| -> Answer<Vec<f64>> {
        let list = value.as_array().ok_or_else(|| malformed("a list"))?;
        let all: Option<Vec<f64>> = list.iter().map(Value::as_f64).collect();
        all.ok_or_else(|| malformed("a number"))
    };
    let field = |key: &str| v.get(key).ok_or_else(|| malformed(key));
    let size = numbers(field("size")?)?;
    let (ku, kv) = (numbers(field("knots_u")?)?, numbers(field("knots_v")?)?);
    let rows = field("heights")?.as_array();
    let rows = rows.ok_or_else(|| malformed("heights"))?;
    if size.len() != 2 || ku.len() < 8 || kv.len() < 8 {
        return Err(malformed("a slab"));
    }
    let (nu, nv) = (ku.len() - 4, kv.len() - 4);
    let over = |knots: &[f64], i: usize| (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0;
    let mut points = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let row = numbers(row)?;
        if rows.len() != nu || row.len() != nv {
            return Err(malformed("a height for each control point"));
        }
        for (j, h) in row.iter().enumerate() {
            points.push(f.place(over(&ku, i) * size[0], over(&kv, j) * size[1], *h));
        }
    }
    let top = shapecraft_core::world::topo::NurbsSurface::new(3, 3, ku, kv, nu, nv, points, None)?;
    Ok(shapecraft_boolean::under(
        "slab", &top, f.origin, f.axes[2],
    )?)
}

/// The constructors that take a sketch: the profile is drawn as one, and
/// the kernel's operation is applied to it.
fn sketched(cmd: &str, v: &Value) -> Answer<Solid> {
    let f = Frame::of(v)?;
    let loops = profile::loops(v.get("profile").ok_or_else(|| malformed("profile"))?)?;
    let [x, y, z] = f.axes;
    let mut b = match cmd {
        "extrude" => world(f.origin, x, y)?,
        // Drawn beside the axis: across is out from it, up is along it.
        _ => world(f.origin, x, z)?,
    };
    let mut sketch = shapecraft_ops_sketch::Sketch::new("sketch");
    sketch.args.plane = Some(Reference::frame_axis("frame", Axis::Z));
    profile::draw(&loops, &mut sketch.args)?;
    sketch.apply(&mut b)?;
    // The suite's profile is one area: its first loop, the others holes
    // in it. A sketch may hold several areas; one that is not the
    // suite's is turned down.
    if let Some(drawn) = b.world.profiles.get("sketch")
        && !drawn.regions.is_empty()
        && !drawn
            .regions
            .iter()
            .any(|area| area.holes.len() + 1 == loops.len())
    {
        return Err(No::Failed(
            "a hole of the profile is not inside its outline".into(),
        ));
    }
    match cmd {
        "extrude" => {
            let mut op = Extrude::of(cmd, Some("sketch".into()), false);
            op.args.distance = Measure::new(number(v, "height")?);
            op.args.combine = Combine::NewBody;
            op.apply(&mut b)?;
        }
        "revolve" => {
            let mut op = Revolve::of(cmd, Some("sketch".into()), false);
            op.args.axis = RevolveAxis::SketchV;
            op.args.angle = Measure::new(number(v, "angle")?.to_degrees());
            op.args.combine = Combine::NewBody;
            op.apply(&mut b)?;
        }
        _ => {
            let rise = number(v, "rise")?;
            let angle = number(v, "angle")?;
            let args = SweptLoftArgs {
                sections: vec![shapecraft_core::Source::Profile("sketch".into()).into()],
                coil: Some(Reference::frame_axis("frame", Axis::Y)),
                pitch: Measure::new(rise.abs() * std::f64::consts::TAU),
                turns: Measure::new(angle / std::f64::consts::TAU),
                left: rise < 0.0,
                combine: Combine::NewBody,
                ..SweptLoftArgs::default()
            };
            let op = SweptLoft {
                id: cmd.to_owned(),
                args,
            };
            op.apply(&mut b)?;
        }
    }
    take(&b, cmd)
}

/// The constructors through sections or along a path.
fn swept(cmd: &str, v: &Value) -> Answer<Solid> {
    let f = Frame::of(v)?;
    let one = |value: &Value, height: f64| -> Answer<Vec<Curve3>> {
        let loops = profile::loops(value)?;
        if loops.len() != 1 {
            return Err(No::Unsupported(
                "a section is one outline: none with holes".into(),
            ));
        }
        profile::curves(&loops[0], &f, height)
    };
    // A profile's area: its outline, and the outlines of its holes.
    let area = |value: &Value| -> Answer<Vec<(Vec<Curve3>, bool)>> {
        let loops = profile::loops(value)?;
        let mut all = Vec::new();
        for (k, segments) in loops.iter().enumerate() {
            all.push((profile::curves(segments, &f, 0.0)?, k > 0));
        }
        Ok(all)
    };
    let field = |key: &str| v.get(key).ok_or_else(|| malformed(key));
    match cmd {
        "loft" => {
            let sections = [
                one(field("profile")?, 0.0)?,
                one(field("top")?, number(v, "height")?)?,
            ];
            Ok(through_sections(cmd, &sections)?)
        }
        "loft_through" => {
            let profiles = field("profiles")?.as_array();
            let profiles = profiles.ok_or_else(|| malformed("profiles"))?;
            let heights = field("heights")?.as_array();
            let heights = heights.ok_or_else(|| malformed("heights"))?;
            if profiles.len() != heights.len() {
                return Err(malformed("a height for each profile"));
            }
            let mut sections = Vec::new();
            for (p, h) in profiles.iter().zip(heights) {
                sections.push(one(p, h.as_f64().ok_or_else(|| malformed("heights"))?)?);
            }
            Ok(through_sections(cmd, &sections)?)
        }
        "sweep" => {
            let outline = area(field("profile")?)?;
            let pieces = field("path")?.as_array();
            let pieces = pieces.ok_or_else(|| malformed("path"))?;
            // The path's pieces as the kernel's curves, each leaving as
            // the last arrived.
            let [mut x, y, mut z] = f.axes;
            let mut at = f.origin;
            let mut path = Vec::new();
            for piece in pieces {
                if let Some(length) = piece.get("line").and_then(Value::as_f64) {
                    let to = at + z * length;
                    path.push(Curve3::line(at, to)?);
                    at = to;
                } else if let Some(arc) = piece.get("arc") {
                    let radius = number(arc, "radius")?;
                    let angle = number(arc, "angle")?;
                    let centre = at + x * radius;
                    path.push(shapecraft_shape::arcs::ellipse(
                        centre,
                        -x * radius,
                        z * radius,
                        angle,
                    ));
                    let (sin, cos) = angle.sin_cos();
                    at = centre - x * radius * cos + z * radius * sin;
                    (x, z) = (x * cos - z * sin, z * cos + x * sin);
                } else {
                    return Err(malformed("a piece of a path"));
                }
            }
            Ok(area_along_path(cmd, &outline, &path, false, y, 0.0, 1.0)?)
        }
        _ => {
            let outline = area(field("profile")?)?;
            let path = field("path")?;
            let degree = path.get("degree").and_then(Value::as_u64);
            let degree = degree.ok_or_else(|| malformed("degree"))? as usize;
            let knots = path.get("knots").and_then(Value::as_array);
            let knots: Option<Vec<f64>> = knots
                .ok_or_else(|| malformed("knots"))?
                .iter()
                .map(Value::as_f64)
                .collect();
            let ctrl: Vec<Point3> = points(path, "ctrl")?
                .into_iter()
                .map(|p| f.place(p.x, p.y, p.z))
                .collect();
            let curve = Curve3::new(degree, knots.ok_or_else(|| malformed("knots"))?, ctrl, None)?;
            let twist = number(v, "twist")?;
            Ok(area_along_path(
                cmd,
                &outline,
                &[curve],
                false,
                // (The suite's sections never turn about the path.)
                shapecraft_shape::swept::Upright::Least(f.axes[1]),
                twist,
                1.0,
            )?)
        }
    }
}

/// Whether edge `e` of `solid` is a seam: where one face closes on
/// itself (a cylinder's side, a turned face). It is how the kernel
/// writes that face and no edge of the solid: no two faces meet there.
fn is_seam(solid: &Solid, e: usize) -> bool {
    use shapecraft_boolean::model::Walk;
    let mut faces = Vec::new();
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if matches!(step.walk, Walk::Edge { edge, .. } if edge == e) {
                faces.push(f);
            }
        }
    }
    faces.len() == 2 && faces[0] == faces[1]
}

/// The edge of `solid` that `p` names: the one the kernel finds `p` on
/// (a seam is not one: see [`is_seam`]).
fn edge_at(solid: &Solid, p: Point3) -> Answer<usize> {
    let found: Vec<usize> = (0..solid.edges.len())
        .filter(|&e| {
            let edge = &solid.edges[e];
            !on_curve(&edge.curve, edge.range, p, NAMES).is_empty() && !is_seam(solid, e)
        })
        .collect();
    match found.as_slice() {
        [] => Err(No::Invalid(format!("no edge passes through {p}"))),
        [one] => Ok(*one),
        several => {
            let names: Vec<&str> = several
                .iter()
                .map(|&e| solid.edges[e].name.as_deref().unwrap_or("?"))
                .collect();
            Err(No::Unsupported(format!(
                "{p} lies on several edges ({}): the adapter does not choose between them",
                names.join(", ")
            )))
        }
    }
}

fn edges_at(solid: &Solid, v: &Value, key: &str) -> Answer<Vec<usize>> {
    let mut all = Vec::new();
    for p in points(v, key)? {
        let e = edge_at(solid, p)?;
        if !all.contains(&e) {
            all.push(e);
        }
    }
    Ok(all)
}

/// The face of `solid` that `p` names: the one the kernel finds `p` on.
fn face_at(solid: &Solid, p: Point3) -> Answer<usize> {
    let mut found = (0..solid.faces.len()).filter(|&f| lies_on(solid, f, p, NAMES));
    let first = found.next();
    let first = first.ok_or_else(|| No::Invalid(format!("no face passes through {p}")))?;
    if found.next().is_some() {
        return Err(No::Unsupported(format!(
            "{p} lies on several faces: the adapter does not choose between them"
        )));
    }
    Ok(first)
}

fn faces_at(solid: &Solid, v: &Value, key: &str) -> Answer<Vec<usize>> {
    let mut all = Vec::new();
    for p in points(v, key)? {
        let f = face_at(solid, p)?;
        if !all.contains(&f) {
            all.push(f);
        }
    }
    Ok(all)
}

fn named(v: &Value, key: &str) -> Answer<Point3> {
    point(v.get(key).ok_or_else(|| malformed(key))?)
}

fn blended(cmd: &str, solid: &Solid, v: &Value) -> Answer<Solid> {
    let round =
        |by: By, size: f64, until: Option<f64>, section: Section, rho: f64| BlendShape::Fillet {
            by,
            size,
            until,
            section,
            rho,
        };
    let (edges, shape) = match cmd {
        "fillet" => (
            edges_at(solid, v, "edges")?,
            round(
                By::Radius,
                number(v, "radius")?,
                None,
                Section::Circular,
                0.5,
            ),
        ),
        "fillet_conic" => (
            edges_at(solid, v, "edges")?,
            round(
                By::Distance,
                number(v, "setback")?,
                None,
                Section::Conic,
                number(v, "rho")?,
            ),
        ),
        "fillet_g2" => (
            edges_at(solid, v, "edges")?,
            round(
                By::Radius,
                number(v, "offset")?,
                None,
                Section::Centred,
                number(v, "center")?,
            ),
        ),
        "fillet_setback" | "fillet_setback_g2" => {
            let section = if cmd == "fillet_setback" {
                Section::Circular
            } else {
                Section::Curvature
            };
            let shape = round(By::Radius, number(v, "radius")?, None, section, 0.5);
            let edges = edges_at(solid, v, "edges")?;
            let setback = number(v, "setback")?;
            return Ok(shapecraft_shape::blend_set_back(
                solid, cmd, &edges, &shape, setback,
            )?);
        }
        "fillet_variable" => {
            let e = edge_at(solid, named(v, "edge")?)?;
            let from = named(v, "from")?;
            let (r_from, r_to) = (number(v, "r_from")?, number(v, "r_to")?);
            // The kernel's round grows from its edge's start: which of
            // the two radii is there is said by `from`.
            let edge = &solid.edges[e];
            let starts = from.distance(edge.point(edge.range.0));
            let ends = from.distance(edge.point(edge.range.1));
            let (first, last) = if starts <= ends {
                (r_from, r_to)
            } else {
                (r_to, r_from)
            };
            (
                vec![e],
                round(By::Radius, first, Some(last), Section::Circular, 0.5),
            )
        }
        "chamfer" => {
            let d = number(v, "distance")?;
            (
                edges_at(solid, v, "edges")?,
                BlendShape::Chamfer {
                    first: d,
                    second: d,
                },
            )
        }
        _ => {
            let e = edge_at(solid, named(v, "edge")?)?;
            let face = face_at(solid, named(v, "face")?)?;
            let (d1, d2) = (number(v, "d1")?, number(v, "d2")?);
            // Which of the edge's two faces the kernel takes first.
            let probe = BlendShape::Chamfer {
                first: d1,
                second: d2,
            };
            let faces = Rolled::of(solid, e, &probe)?.faces;
            let shape = if faces[0] == face {
                probe
            } else if faces[1] == face {
                BlendShape::Chamfer {
                    first: d2,
                    second: d1,
                }
            } else {
                return Err(No::Invalid("the face does not meet the edge".into()));
            };
            (vec![e], shape)
        }
    };
    // A fillet or a chamfer runs along the whole tangent chain of each
    // edge named: the kernel follows it.
    Ok(match cmd {
        "fillet" | "fillet_conic" | "fillet_g2" | "chamfer" => {
            blend_chains(solid, cmd, &edges, &shape, true)?
        }
        _ => blend_solid(solid, cmd, &edges, &shape, true)?,
    })
}

fn edited(cmd: &str, solid: &Solid, v: &Value) -> Answer<Solid> {
    use shapecraft_shape::thick;
    Ok(match cmd {
        "offset" => thick::thickened(solid, number(v, "distance")?, None, false)?,
        "offset_faces" => {
            let faces = faces_at(solid, v, "faces")?;
            thick::faces_offset(solid, &faces, number(v, "distance")?)?
        }
        "delete_faces" => {
            let faces = faces_at(solid, v, "faces")?;
            shapecraft_ops_faces::faces_deleted(solid, &faces)?
        }
        "move_faces" => {
            let faces = faces_at(solid, v, "faces")?;
            let (m, t) = matrix(v)?;
            thick::faces_placed(solid, &faces, &|p| m * p + t)?
        }
        "thicken" => {
            let faces = faces_at(solid, v, "faces")?;
            thick::thicken(solid, &faces, number(v, "thickness")?, false, false)?
        }
        "shell" => {
            let open = faces_at(solid, v, "open")?;
            thick::shelled(solid, number(v, "thickness")?, &open)?
        }
        _ => {
            let faces = faces_at(solid, v, "faces")?;
            let origin = named(v, "origin")?;
            let pull = named(v, "pull")?;
            thick::drafted(solid, &faces, pull, origin, number(v, "angle")?, false)?
        }
    })
}

/// The `[M | t]` of a request.
fn matrix(v: &Value) -> Answer<(DMat3, Point3)> {
    let rows = v.get("matrix").and_then(Value::as_array);
    let rows = rows
        .filter(|r| r.len() == 3)
        .ok_or_else(|| malformed("matrix"))?;
    let mut m = [[0.0; 3]; 3];
    let mut t = [0.0; 3];
    for (i, row) in rows.iter().enumerate() {
        let row = row.as_array().filter(|r| r.len() == 4);
        let row = row.ok_or_else(|| malformed("a row of the matrix"))?;
        for (j, x) in row.iter().enumerate() {
            let x = x
                .as_f64()
                .ok_or_else(|| malformed("a number of the matrix"))?;
            if j < 3 {
                m[i][j] = x;
            } else {
                t[i] = x;
            }
        }
    }
    // (glam's matrices are given by their columns.)
    let m = DMat3::from_cols_array_2d(&m).transpose();
    Ok((m, Point3::from_array(t)))
}

/// `solid` moved by a similarity: the kernel takes the map with whether
/// it mirrors and by how much it scales.
fn moved(solid: &Solid, v: &Value) -> Answer<Solid> {
    let (m, t) = matrix(v)?;
    let det = m.determinant();
    let scale = det.abs().cbrt();
    Ok(solid.mapped(&|p| m * p + t, det < 0.0, scale)?)
}

fn reply(worker: &mut Worker, line: &str) -> Value {
    let request = match serde_json::from_str::<Request>(line) {
        Ok(Request { rest, points }) => {
            POINTS.with(|p| *p.borrow_mut() = points);
            rest
        }
        Err(e) => return json!({"err": format!("not JSON: {e}"), "kind": "failed"}),
    };
    let Some(cmd) = request.get("cmd").and_then(Value::as_str) else {
        return json!({"err": "a request has a cmd", "kind": "failed"});
    };
    if cmd != "hello" && cmd != "reset" && !SUPPORTS.contains(&cmd) {
        return json!({"err": format!("shapecraft has no call for {cmd}"), "kind": "unsupported"});
    }
    match catch_unwind(AssertUnwindSafe(|| worker.answer(cmd, &request))) {
        Ok(Ok(value)) => json!({"ok": value}),
        Ok(Err(No::Unsupported(why))) => json!({"err": why, "kind": "unsupported"}),
        Ok(Err(No::Invalid(why))) => json!({"err": why, "kind": "invalid"}),
        Ok(Err(No::Failed(why))) => json!({"err": why, "kind": "failed"}),
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "the kernel panicked".to_owned());
            json!({"crash": message})
        }
    }
}

fn main() {
    let mut worker = Worker::default();
    let times = std::env::var_os("SHAPECRAFT_TIMES").is_some();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let started = std::time::Instant::now();
        let answer = reply(&mut worker, &line);
        if times {
            // (For profiling: each call with how long it took.)
            let cmd = line.split("\"cmd\":").nth(1).unwrap_or("");
            let cmd = cmd.split('"').nth(1).unwrap_or("?");
            eprintln!("TIME {cmd} {:.3}", started.elapsed().as_secs_f64() * 1e3);
        }
        if writeln!(stdout, "{answer}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}
