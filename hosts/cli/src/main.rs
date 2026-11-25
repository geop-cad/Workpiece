//! `workpiece`: the command line scripts and agents build parts with.
//!
//! `workpiece make recipe.json -o part.workpiece [--report report.json]
//! [--render dir]` makes a part of a recipe — its steps said compactly,
//! sketches by named points and curves with their constraints and
//! dimensions, every other step by its kind and the arguments that are
//! not as by default — solves its sketches, builds it step by step, and
//! says what came of it: which steps failed and why, what every sketch
//! can still do, the bodies and their volumes after every step, the
//! faces and edges of the result by name, and what in it does not look
//! like a person made it. `workpiece report part.workpiece` says the same of a
//! part there is; `workpiece kinds` lists the kinds of steps with their
//! arguments as by default.

use std::collections::BTreeMap;

use anyhow::{Context, bail, ensure};
use serde_json::{Value, json};
use workpiece_core::math::{Axis, Point2, Point3};
use workpiece_core::world::topo::BodyKind;
use workpiece_core::{Reference, World};
use workpiece_ops_sketch::{Kind, Shape, Sketch, SketchAxis};
use workpiece_part::{AnyOperation, Part, Workspace, registry};

mod inventory;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        let at = args.iter().position(|a| a == name)?;
        args.get(at + 1).cloned()
    };
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("workpiece {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h" | "help") => {
            println!("workpiece {}\n\n{USAGE}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("kinds") => {
            println!("{}", serde_json::to_string_pretty(&defaults())?);
            Ok(())
        }
        Some("materials") => {
            // The materials of a folder (its library file, or those a
            // library starts with): what is added to it is written back.
            let dir = std::path::PathBuf::from(flag("--in").unwrap_or_else(|| ".".to_owned()));
            print!("{}", library_in(&dir)?.to_text()?);
            Ok(())
        }
        Some("make") => {
            let from = args.get(1).context("which recipe?")?;
            let recipe: Value = serde_json::from_str(&std::fs::read_to_string(from)?)?;
            let out = flag("-o").context("where to (-o part.workpiece)?")?;
            let (part, names) = made(&recipe, &out)?;
            std::fs::write(&out, part.to_text()?)?;
            // The drawings that are to show all that is said of the model.
            let all_notes: BTreeMap<String, Value> = recipe["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s["place_notes"].as_bool() == Some(true))
                .filter_map(|s| Some((s["id"].as_str()?.to_owned(), s.clone())))
                .collect();
            said(
                &out,
                part,
                &names,
                &all_notes,
                flag("--report"),
                flag("--render"),
            )
        }
        Some("report") => {
            let path = args.get(1).context("which part?")?;
            let part = Part::from_text(&std::fs::read_to_string(path)?)?;
            said(
                path,
                part,
                &BTreeMap::new(),
                &BTreeMap::new(),
                flag("--report"),
                flag("--render"),
            )
        }
        _ => bail!("{USAGE}"),
    }
}

/// What the command line says of itself.
const USAGE: &str = "usage:
  workpiece make recipe.json -o part.workpiece [--report r.json] [--render dir [--no-pictures]] [--times]
                  [--picture file.png [--size 1200x900] [--samples 256] [--seconds 5]]
  workpiece report part.workpiece [--report r.json] [--render dir]
  workpiece kinds                      the kinds of steps, with their arguments as by default
  workpiece materials [--in folder]
  workpiece --version";

/// Every kind of step a tool makes, as the tool makes it: what its
/// arguments are when nothing else is said.
fn defaults() -> BTreeMap<String, Value> {
    let world = World::new();
    let mut all = BTreeMap::new();
    for tool in registry().tools() {
        let context = workpiece_core::registry::ToolContext {
            fresh_id: &|name| name.to_owned(),
            world: &world,
            unit: workpiece_core::Unit::default(),
            selection: &[],
        };
        let made: AnyOperation = (tool.make)(&context);
        if let Ok(Value::Object(map)) = serde_json::to_value(&made) {
            for (kind, value) in map {
                all.entry(kind).or_insert(value);
            }
        }
    }
    all
}

/// `over` laid over `under`: objects field by field, anything else
/// whole.
fn overlaid(under: &mut Value, over: &Value) {
    match (under, over) {
        (Value::Object(under), Value::Object(over)) => {
            for (key, value) in over {
                overlaid(under.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (under, over) => *under = over.clone(),
    }
}

/// Fails if a step's `given` arguments name one its kind does not have:
/// an argument that is silently dropped makes a part other than the one
/// meant. The arguments a kind has are those it is by default and those
/// it says when made (a few are left out of its saying while they are
/// empty, so an empty value is let by). Only the arguments themselves
/// are checked, not what lies inside them (an enum's variants, a map).
fn refuse_unknown(
    n: usize,
    kind: &str,
    id: &str,
    given: &serde_json::Map<String, Value>,
    default: &Value,
    made: &AnyOperation,
) -> anyhow::Result<()> {
    let mut valid: std::collections::BTreeSet<String> = default
        .as_object()
        .map(|d| d.keys().cloned().collect())
        .unwrap_or_default();
    if let Ok(Value::Object(said)) = serde_json::to_value(made)
        && let Some(Value::Object(args)) = said.values().next().map(|v| &v["args"])
    {
        valid.extend(args.keys().cloned());
    }
    let empty = |v: &Value| match v {
        Value::Null | Value::Bool(false) => true,
        Value::Array(a) => a.is_empty(),
        _ => false,
    };
    for (key, value) in given {
        ensure!(
            valid.contains(key) || empty(value),
            "step {n} ({kind} {id}): {kind} has no argument {key:?}; its arguments are: {}",
            valid.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

/// The names a recipe gave the points and curves of each sketch, by
/// their ids in it.
type Names = BTreeMap<String, Named>;

/// The names of a sketch's points and of its curves, by their ids.
pub(crate) type Named = (
    BTreeMap<u32, String>,
    BTreeMap<u32, String>,
    BTreeMap<u32, String>,
    BTreeMap<String, Value>,
);

/// The part a recipe says, with its sketches solved; `path` is where
/// it is to be written: the folder it is in is what every build on the
/// way is made in (the parts it places, its materials).
fn made(recipe: &Value, path: &str) -> anyhow::Result<(Part, Names)> {
    let defaults = defaults();
    let mut program = Vec::new();
    let mut names = Names::new();
    let mut poses: Vec<(String, workpiece_core::Pose)> = Vec::new();
    let steps = recipe["steps"].as_array().context("a recipe has steps")?;
    // (One engine for what sketches project from: what is built for one
    // is not built again for the next.)
    let engine = workpiece_part::Engine::inline();
    let cancel = workpiece_part::Cancel::new();
    for (n, step) in steps.iter().enumerate() {
        let made = if let Some(sketch) = step.get("sketch") {
            // What it projects of the model is of the model as the steps
            // before it leave it.
            let world = match sketch.get("project").and_then(Value::as_object) {
                Some(wanted) if !wanted.is_empty() => {
                    let mut so_far = Part::new(program.clone());
                    for (id, pose) in &poses {
                        so_far
                            .cache
                            .replace(id, workpiece_ops_instances::assembly::kept(pose));
                    }
                    let workspace = workspace_of(path, so_far)?;
                    Some(
                        engine
                            .build(
                                &std::sync::Arc::new(workspace),
                                path,
                                None,
                                &|_| true,
                                &cancel,
                            )
                            .map(|built| built.world)
                            .with_context(|| format!("step {n}: building what it projects from"))?,
                    )
                }
                _ => None,
            };
            let (sketch, named) = sketched(sketch, world.as_deref())
                .with_context(|| format!("step {n}, a sketch"))?;
            names.insert(sketch.id.clone(), named);
            AnyOperation::from(sketch)
        } else {
            let kind = step["op"]
                .as_str()
                .with_context(|| format!("step {n}: \"op\" or \"sketch\""))?;
            let id = step["id"]
                .as_str()
                .with_context(|| format!("step {n}: its \"id\""))?;
            let mut value = defaults
                .get(kind)
                .cloned()
                .unwrap_or_else(|| json!({ "args": {} }));
            value["id"] = json!(id);
            let by_default = value["args"].clone();
            if let Some(args) = step.get("args") {
                overlaid(&mut value["args"], args);
            }
            by_sketch_names(&mut value["args"], &names);
            let load = registry()
                .kinds()
                .find(|k| k.name == kind)
                .with_context(|| {
                    format!("step {n}: there is no kind of step {kind:?} (see `workpiece kinds`)")
                })?
                .load;
            if let Some(pose) = step.get("pose").filter(|p| !p.is_null()) {
                // Where a part it places is put (for one held "Fixed" or
                // "Free"): kept where the app keeps it, in the cache.
                poses.push((
                    id.to_owned(),
                    pose_of(pose).with_context(|| format!("step {n} ({id}): its pose"))?,
                ));
            }
            let made = load(value.clone())
                .with_context(|| format!("step {n} ({kind} {id}): its arguments"))?;
            if let Some(given) = step.get("args").and_then(Value::as_object) {
                refuse_unknown(n, kind, id, given, &by_default, &made)?;
            }
            made
        };
        program.push(made);
    }
    // Two steps of one name: what refers to the name would mean either.
    let mut taken = std::collections::BTreeSet::new();
    for op in &program {
        ensure!(
            taken.insert(op.id().to_owned()),
            "two steps are called {:?}: every step (sketches too) needs a name of its own",
            op.id()
        );
    }
    let mut part = Part::new(program);
    if let Some(unit) = recipe.get("unit").filter(|u| !u.is_null()) {
        part.unit = serde_json::from_value(unit.clone()).context("the unit")?;
    }
    part.material = recipe["material"].as_str().map(str::to_owned);
    // (What the part says of itself: its title, ...)
    for (key, value) in recipe["properties"].as_object().into_iter().flatten() {
        if let Some(value) = value.as_str() {
            part.properties.insert(key.clone(), value.to_owned());
        }
    }
    for (id, pose) in poses {
        part.cache
            .replace(&id, workpiece_ops_instances::assembly::kept(&pose));
    }
    Ok((part, names))
}

/// `value` with each datum a recipe names by a sketch's name for one of
/// its curves (`{"Datum": "sketch1/axis"}`, a rotation axis drawn in it)
/// named as the sketch names the datum it makes of it (`sketch1/e7`).
fn by_sketch_names(value: &mut Value, names: &Names) {
    match value {
        Value::Object(map) => {
            if let (1, Some(Value::String(name))) = (map.len(), map.get_mut("Datum"))
                && let Some((sketch, curve)) = name.split_once('/')
                && let Some(id) = names
                    .get(sketch)
                    .and_then(|named| named.1.iter().find(|(_, n)| *n == curve))
                    .map(|(id, _)| *id)
            {
                *name = format!("{sketch}/e{id}");
            }
            for inner in map.values_mut() {
                by_sketch_names(inner, names);
            }
        }
        Value::Array(all) => {
            for inner in all {
                by_sketch_names(inner, names);
            }
        }
        _ => {}
    }
}

/// A pose a recipe says: `{"at": [x, y, z], "euler": [x, y, z]}` — where
/// the placed part's origin goes, and how it is turned (degrees about
/// x, y and z, as the app's pose fields have them).
fn pose_of(said: &Value) -> anyhow::Result<workpiece_core::Pose> {
    let three = |key: &str| -> anyhow::Result<[f64; 3]> {
        match said.get(key) {
            None | Some(Value::Null) => Ok([0.0; 3]),
            Some(v) => {
                let v: Vec<f64> = serde_json::from_value(v.clone())
                    .with_context(|| format!("{key:?} is three numbers"))?;
                <[f64; 3]>::try_from(v).map_err(|_| anyhow::anyhow!("{key:?} is three numbers"))
            }
        }
    };
    Ok(workpiece_core::Pose::IDENTITY
        .with_euler_degrees(three("euler")?)
        .with_translation(Point3::from_array(three("at")?)))
}

/// The plane a recipe names: `"XY"`, `"YZ"`, `"ZX"` of the origin, or a
/// reference as every step's are — a datum (`{"Datum": name}`), a flat
/// face (`{"Topology": name}`), a frame's plane.
fn plane(said: &Value) -> anyhow::Result<Reference<workpiece_core::world::DatumPlane>> {
    let told = "the plane is \"XY\", \"YZ\", \"ZX\", datum(name) or topo(face)";
    Ok(match said {
        Value::String(s) => match s.to_uppercase().as_str() {
            "XY" => Reference::frame_axis("origin", Axis::Z),
            "YZ" => Reference::frame_axis("origin", Axis::X),
            "ZX" | "XZ" => Reference::frame_axis("origin", Axis::Y),
            _ => bail!("{told}"),
        },
        Value::Object(_) => serde_json::from_value(said.clone()).context(told)?,
        _ => bail!("{told}"),
    })
}

/// The sketch a recipe says, solved; and the names of its points and
/// curves.
fn sketched(said: &Value, world: Option<&World>) -> anyhow::Result<(Sketch, Named)> {
    let id = said["id"].as_str().context("its \"id\"")?;
    let mut sketch = Sketch::new(id);
    sketch.args.plane = Some(plane(&said["plane"])?);
    // What it projects of the model: fixed construction geometry.
    let mut projected: Vec<(String, u32)> = Vec::new();
    for (name, source) in said
        .get("project")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let world = world.context("nothing is built yet to project from")?;
        let source: workpiece_core::Source = serde_json::from_value(source.clone())
            .with_context(|| format!("what {name:?} is the projection of"))?;
        let made = sketch
            .project(world, &source)
            .with_context(|| format!("projecting {name:?}"))?;
        let first = *made
            .first()
            .with_context(|| format!("{name:?} projects to nothing"))?;
        projected.push((name.clone(), first));
    }
    let args = &mut sketch.args;
    // What every sketch has: its origin and the plane's two axes.
    let mut points: BTreeMap<String, u32> = BTreeMap::new();
    let mut curves: BTreeMap<String, u32> = BTreeMap::new();
    for (id, entity) in args.entities.iter().filter(|(_, e)| e.projected.is_none()) {
        match (&entity.shape, entity.axis) {
            (Shape::Point(p), _) => {
                points.insert("origin".to_owned(), *p);
            }
            (_, Some(SketchAxis::U)) => {
                curves.insert("u".to_owned(), *id);
            }
            (_, Some(SketchAxis::V)) => {
                curves.insert("v".to_owned(), *id);
            }
            _ => {}
        }
    }
    // The projections by name: a point as a point; a curve as a curve,
    // with its points as `name_a`, `name_b` (a line's ends), `name_center`,
    // `name_start`, `name_end` (an arc's, a circle's).
    for (name, id) in projected {
        let ends: Vec<(&str, u32)> = match &args.entities[&id].shape {
            Shape::Point(p) => {
                points.insert(name.clone(), *p);
                Vec::new()
            }
            Shape::Line(a, b) => vec![("a", *a), ("b", *b)],
            Shape::Circle { center, .. } => vec![("center", *center)],
            Shape::Arc { center, start, end } => {
                vec![("center", *center), ("start", *start), ("end", *end)]
            }
            Shape::Spline { .. } | Shape::Axis { .. } | Shape::Text(_) => Vec::new(),
        };
        if !matches!(args.entities[&id].shape, Shape::Point(_)) {
            curves.insert(name.clone(), id);
        }
        for (end, point) in ends {
            points.insert(format!("{name}_{end}"), point);
        }
    }
    let object = |key: &str| {
        said.get(key)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    let number = |v: &Value| v.as_f64().context("a number");
    for (name, at) in object("points") {
        let at = Point2::new(number(&at[0])?, number(&at[1])?);
        ensure!(
            !points.contains_key(&name),
            "two points are called {name:?}"
        );
        points.insert(name, args.add_point(at));
    }
    let point = |points: &BTreeMap<String, u32>, v: &Value| -> anyhow::Result<u32> {
        let name = v.as_str().context("a point's name")?;
        points
            .get(name)
            .copied()
            .with_context(|| format!("there is no point {name:?}"))
    };
    for (name, ends) in object("lines") {
        let line = args.add_line(point(&points, &ends[0])?, point(&points, &ends[1])?);
        curves.insert(name, line);
    }
    for (name, circle) in object("circles") {
        let made = args.add(Shape::Circle {
            center: point(&points, &circle["center"])?,
            radius: number(&circle["radius"])?,
        });
        curves.insert(name, made);
    }
    for (name, arc) in object("arcs") {
        // Counter-clockwise from its start to its end; "cw" says it the
        // other way around.
        let (mut start, mut end) = (point(&points, &arc["start"])?, point(&points, &arc["end"])?);
        if arc["cw"].as_bool() == Some(true) {
            std::mem::swap(&mut start, &mut end);
        }
        let made = args.add(Shape::Arc {
            center: point(&points, &arc["center"])?,
            start,
            end,
        });
        curves.insert(name, made);
    }
    // The axes the sketch is turned about: each through a point, towards
    // another (which it passes through as well) or along a direction.
    for (name, axis) in object("rotation_axes") {
        let through = point(&points, &axis["through"])?;
        let towards = &axis["towards"];
        let (direction, also) = match towards.as_array() {
            Some(d) => (Point2::new(number(&d[0])?, number(&d[1])?), None),
            None => {
                let to = point(&points, towards).with_context(|| {
                    format!("rotation axis {name:?}: towards a point or (u, v)")
                })?;
                (args.at(to) - args.at(through), Some(to))
            }
        };
        ensure!(
            direction.length() > 1e-12,
            "rotation axis {name:?} has no direction"
        );
        let made = args.add_rotation_axis(through, direction);
        if let Some(point) = also {
            args.constrain(Kind::PointOn {
                point,
                entity: made,
            });
        }
        curves.insert(name, made);
    }
    // Points that stand for themselves: where holes go.
    for name in said["dots"].as_array().into_iter().flatten() {
        let id = point(&points, name)?;
        args.add(Shape::Point(id));
    }
    let curve = |curves: &BTreeMap<String, u32>, v: &Value| -> anyhow::Result<u32> {
        let name = v.as_str().context("a curve's name")?;
        curves
            .get(name)
            .copied()
            .with_context(|| format!("there is no line, circle or arc {name:?}"))
    };
    for name in said["construction"].as_array().into_iter().flatten() {
        let id = curve(&curves, name)?;
        args.entities.get_mut(&id).expect("just added").construction = true;
    }
    for name in said["fixed"].as_array().into_iter().flatten() {
        let id = point(&points, name)?;
        args.points.get_mut(&id).expect("just added").fixed = true;
    }
    let mut constraints: BTreeMap<u32, String> = BTreeMap::new();
    let mut on_sheet: BTreeMap<String, Value> = BTreeMap::new();
    let sketch_id = id.to_owned();
    for (n, c) in said["constraints"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let what = c[0]
            .as_str()
            .with_context(|| format!("constraint {n}: its kind first"))?;
        let p =
            |k: usize| point(&points, &c[k]).with_context(|| format!("constraint {n} ({what})"));
        let e =
            |k: usize| curve(&curves, &c[k]).with_context(|| format!("constraint {n} ({what})"));
        let last = c.as_array().and_then(|a| a.last()).and_then(Value::as_f64);
        let value = || last.with_context(|| format!("constraint {n} ({what}): its value last"));
        let axis = |name: &str| {
            if name.ends_with('v') || name.starts_with('v') {
                SketchAxis::V
            } else {
                SketchAxis::U
            }
        };
        let middle = |a: u32, b: u32| (args.at(a) + args.at(b)) / 2.0;
        let (kind, dimension) = match what {
            "coincident" => (Kind::Coincident(p(1)?, p(2)?), None),
            "point_on" => (
                Kind::PointOn {
                    point: p(1)?,
                    entity: e(2)?,
                },
                None,
            ),
            "horizontal" | "vertical" => {
                // Of a line (its ends), of two points, or of a rotation
                // axis (along the plane's axis).
                let axis = if what == "horizontal" {
                    SketchAxis::U
                } else {
                    SketchAxis::V
                };
                let kind = match e(1) {
                    Ok(line) => match args.entities[&line].shape {
                        Shape::Line(a, b) => Kind::Along { a, b, axis },
                        Shape::Axis { .. } => {
                            let level = args.plane_axis(axis).context("the plane's axis")?;
                            Kind::Parallel(line, level)
                        }
                        _ => bail!("constraint {n}: {what} is of a line or of two points"),
                    },
                    Err(_) => Kind::Along {
                        a: p(1)?,
                        b: p(2)?,
                        axis,
                    },
                };
                (kind, None)
            }
            "parallel" => (Kind::Parallel(e(1)?, e(2)?), None),
            "perpendicular" => (Kind::Perpendicular(e(1)?, e(2)?), None),
            "collinear" => (Kind::Collinear(e(1)?, e(2)?), None),
            "tangent" => (
                Kind::Tangent {
                    a: e(1)?,
                    b: e(2)?,
                    internal: c[3].as_bool() == Some(true),
                },
                None,
            ),
            "equal" => (Kind::Equal(e(1)?, e(2)?), None),
            "midpoint" => (
                Kind::Midpoint {
                    point: p(1)?,
                    line: e(2)?,
                },
                None,
            ),
            "symmetric" => (
                Kind::Symmetric {
                    a: p(1)?,
                    b: p(2)?,
                    line: e(3)?,
                },
                None,
            ),
            "distance" | "distance_far" => {
                // Between two points; or, measured to the side of what
                // is round: from a point or a line to a circle or arc
                // (its near side, or with "distance_far" its far one),
                // or between two circles where they are nearest.
                let far = what == "distance_far";
                let round = |id: u32| {
                    matches!(
                        args.entities[&id].shape,
                        Shape::Circle { .. } | Shape::Arc { .. }
                    )
                };
                let label = |id: u32| {
                    args.circle(id)
                        .map_or(Point2::ZERO, |(_, c, r)| c + Point2::new(r, r))
                };
                match (p(1), p(2), e(1), e(2)) {
                    (Ok(a), Ok(b), ..) => (Kind::Distance(a, b), Some((value()?, middle(a, b)))),
                    (Ok(point), _, _, Ok(to)) | (_, Ok(point), Ok(to), _) if round(to) => (
                        Kind::PointRound {
                            point,
                            round: to,
                            far,
                        },
                        Some((value()?, label(to))),
                    ),
                    (_, _, Ok(a), Ok(b)) if round(a) && round(b) => {
                        (Kind::Gap(a, b), Some((value()?, label(a))))
                    }
                    (_, _, Ok(a), Ok(b)) if round(a) != round(b) => {
                        let (line, to) = if round(a) { (b, a) } else { (a, b) };
                        (
                            Kind::LineRound {
                                line,
                                round: to,
                                far,
                            },
                            Some((value()?, label(to))),
                        )
                    }
                    _ => bail!(
                        "constraint {n} ({what}): between two points, a point or a line and a circle or arc, or two circles (for a point and a line: point_line)"
                    ),
                }
            }
            "diameter_about" => (
                Kind::PointLine {
                    point: p(1)?,
                    line: e(2)?,
                },
                Some((value()?, args.at(p(1)?))),
            ),
            "distance_u" | "distance_v" => {
                let (a, b) = (p(1)?, p(2)?);
                (
                    Kind::DistanceAlong {
                        a,
                        b,
                        axis: axis(&what[9..]),
                    },
                    Some((value()?, middle(a, b))),
                )
            }
            "point_line" => (
                Kind::PointLine {
                    point: p(1)?,
                    line: e(2)?,
                },
                Some((value()?, args.at(p(1)?))),
            ),
            "radius" | "diameter" => {
                let of = e(1)?;
                let at = args
                    .circle(of)
                    .map_or(Point2::ZERO, |(_, c, r)| c + Point2::new(r, r) * 0.8);
                (Kind::Size(of), Some((value()?, at)))
            }
            "angle" => {
                // Its text between the two lines, off where they cross.
                let (a, b) = (e(1)?, e(2)?);
                let label = match (args.line(a), args.line(b)) {
                    (Some((a0, a1)), Some((b0, b1))) => {
                        let (u, v) = (a1 - a0, b1 - b0);
                        let cross = u.perp_dot(v);
                        let at = if cross.abs() > 1e-12 {
                            a0 + u * ((b0 - a0).perp_dot(v) / cross)
                        } else {
                            a0
                        };
                        let reach = u.length().min(v.length()) * 0.5;
                        at + (u.normalize_or_zero() + v.normalize_or_zero()).normalize_or_zero()
                            * reach
                    }
                    _ => Point2::ZERO,
                };
                (Kind::Angle(a, b), Some((value()?, label)))
            }
            other => bail!(
                "constraint {n}: there is no {other:?} (coincident, point_on, horizontal, vertical, parallel, \
                 perpendicular, collinear, tangent, equal, midpoint, symmetric, distance, distance_far, distance_u, \
                 distance_v, point_line, radius, diameter, diameter_about, angle)"
            ),
        };
        // (Of a circle, or about a line as the axis: a diameter.)
        let diameter = matches!(what, "diameter" | "diameter_about");
        let id = match dimension {
            Some((value, label)) => args.dimension_of(kind, value, label, diameter),
            None => args.constrain(kind),
        };
        constraints.insert(id, c.to_string());
        // What the recipe says of where this dimension goes on drawings,
        // by the name its note has there.
        if let Some(how) = said["dimension_options"].get(n.to_string()) {
            // (Named by the recipe, it is called so: `sketch/width`.)
            let name = how["id"].as_str().map(str::to_owned);
            if let Some(d) = args
                .constraints
                .get_mut(&id)
                .and_then(|c| c.dimension.as_mut())
            {
                d.name.clone_from(&name);
            }
            let said_as = match &name {
                Some(name) => format!("{sketch_id}/{name}"),
                None => format!("{sketch_id}/d{id}"),
            };
            on_sheet.insert(said_as, how.clone());
            // For reference: it holds nothing, and says what is (driven).
            if how["reference"].as_bool() == Some(true) {
                args.set_driven(id, true);
            }
        }
    }
    args.solve();
    let back =
        |named: BTreeMap<String, u32>| named.into_iter().map(|(name, id)| (id, name)).collect();
    Ok((sketch, (back(points), back(curves), constraints, on_sheet)))
}

/// Builds `part` step by step and says what came of it: on the
/// terminal in short, in `report` in full, in `render` as pictures.
fn said(
    path: &str,
    part: Part,
    names: &Names,
    all_notes: &BTreeMap<String, Value>,
    report: Option<String>,
    render: Option<String>,
) -> anyhow::Result<()> {
    let ids: Vec<String> = part.program.iter().map(|op| op.id().to_owned()).collect();
    let sketches: BTreeMap<String, Sketch> = part
        .program
        .iter()
        .filter_map(|op| {
            op.downcast_ref::<Sketch>()
                .map(|s| (s.id.clone(), s.clone()))
        })
        .collect();
    let kinds: Vec<&str> = part.program.iter().map(AnyOperation::kind).collect();
    let workspace = std::sync::Arc::new(workspace_of(path, part)?);
    // One engine for all of it: a step built once is not built again
    // for the steps after it.
    let engine = workpiece_part::Engine::inline();
    let cancel = workpiece_part::Cancel::new();
    let build_steps = |steps: usize| {
        engine
            .build(&workspace, path, None, &|i| i < steps, &cancel)
            .map(|built| built.world)
    };
    let times = std::env::args().any(|a| a == "--times");
    let clock = std::time::Instant::now();
    let mut lap = clock.elapsed();
    let mut timed = |what: &str| {
        let now = clock.elapsed();
        if times {
            println!("time    {:>8.3} s  {what}", (now - lap).as_secs_f64());
        }
        lap = now;
    };
    let mut steps = Vec::new();
    let mut lint = Vec::new();
    let mut last: Option<std::sync::Arc<World>> = None;
    let mut failed = 0;
    for n in 0..ids.len() {
        let id = &ids[n];
        let mut step = json!({ "id": id, "kind": kinds[n] });
        let built = build_steps(n + 1);
        timed(&format!("step {id}"));
        match built {
            Ok(world) => {
                step["ok"] = json!(true);
                step["volumes"] = json!(volumes(&world));
                // What a dimension step measured, as the sheet writes it.
                if let Some(said) = world.annotations.get(id.as_str()) {
                    step["says"] = json!(said.text());
                }
                println!("ok      {id}");
                last = Some(world);
            }
            Err(error) => {
                failed += 1;
                step["ok"] = json!(false);
                step["error"] = json!(format!("{error:#}"));
                println!("FAILED  {id}: {error:#}");
            }
        }
        if let Some(sketch) = sketches.get(id) {
            let d = sketch.args.diagnose();
            let (points, curves, constraints, _) = names.get(id).cloned().unwrap_or_default();
            let called = |ids: &std::collections::BTreeSet<u32>,
                          of: &BTreeMap<u32, String>|
             -> Vec<String> {
                ids.iter()
                    .map(|i| of.get(i).cloned().unwrap_or_else(|| format!("#{i}")))
                    .collect()
            };
            // Where the solver left everything: to see what a sketch
            // became without opening it.
            let at: BTreeMap<&String, [f64; 2]> = points
                .iter()
                .map(|(i, name)| (name, sketch.args.at(*i).to_array()))
                .collect();
            let radii: BTreeMap<&String, f64> = curves
                .iter()
                .filter_map(|(i, name)| Some((name, sketch.args.circle(*i)?.2)))
                .collect();
            step["sketch"] = json!({
                "solved": d.solved,
                "degrees_of_freedom": d.dof,
                "free_points": called(&d.free_points, &points),
                "free_radii": called(&d.free_entities, &curves),
                "redundant": called(&d.redundant, &constraints),
                "conflicting": called(&d.conflicting, &constraints),
                "dimensions": sketch.args.constraints.values().filter(|c| c.kind.is_dimension()).count(),
                "points": at,
                "radii": radii,
            });
            if !d.solved || !d.conflicting.is_empty() {
                lint.push(format!(
                    "sketch {id}: its constraints do not hold together (in conflict: {})",
                    called(&d.conflicting, &constraints).join(" ")
                ));
            } else if d.dof > 0 {
                let free = called(&d.free_points, &points).join(", ");
                lint.push(format!(
                    "sketch {id} is not fully constrained: {} degrees of freedom (can still move: {free})",
                    d.dof
                ));
            }
            if !d.redundant.is_empty() {
                lint.push(format!(
                    "sketch {id} says some things twice (redundant: {})",
                    called(&d.redundant, &constraints).join(" ")
                ));
            }
        }
        steps.push(step);
    }
    // Steps of one kind that say the same but for what they are of:
    // one feature made again and again by hand.
    let mut alike: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (n, op) in workspace.part(path)?.program.iter().enumerate() {
        if op.is::<Sketch>() {
            continue;
        }
        let Ok(Value::Object(map)) = serde_json::to_value(op) else {
            continue;
        };
        let Some(mut args) = map.into_iter().next().map(|(_, v)| v["args"].clone()) else {
            continue;
        };
        // What its sketch is dimensioned to: steps of sketches that
        // differ in their numbers are different things, not one thing
        // made again.
        let sized = args["sketch"]["Profile"]
            .as_str()
            .and_then(|id| sketches.get(id))
            .map(|sketch| {
                let mut values: Vec<String> = sketch
                    .args
                    .constraints
                    .values()
                    .filter_map(|c| c.dimension.as_ref().map(|d| format!("{:.6}", d.value)))
                    .collect();
                values.sort();
                values.join(",")
            })
            .unwrap_or_default();
        for key in ["sketch", "plane", "references", "at"] {
            if let Some(o) = args.as_object_mut() {
                o.remove(key);
            }
        }
        alike
            .entry(format!("{} {args} {sized}", kinds[n]))
            .or_default()
            .push(&ids[n]);
    }
    for (what, same) in alike {
        if same.len() >= 3 {
            let kind = what.split(' ').next().unwrap_or_default();
            lint.push(format!(
                "{} {kind} steps say the same but for their sketch ({}): one feature and a pattern would say it once",
                same.len(),
                same.join(", ")
            ));
        }
    }
    // What each step says of what it refers to, for the sheet's inventory.
    let step_args: BTreeMap<String, Value> = workspace
        .part(path)?
        .program
        .iter()
        .filter_map(|op| {
            let Ok(Value::Object(map)) = serde_json::to_value(op) else {
                return None;
            };
            let args = map.into_iter().next()?.1["args"].clone();
            Some((op.id().to_owned(), args))
        })
        .collect();
    // The drawings: each laid out from the model as the steps before
    // it leave it, and written as PDF and SVG beside the pictures.
    let mut drawings = Vec::new();
    let mut part = workspace.part(path)?.clone();
    let mut placed_any = false;
    #[allow(clippy::needless_range_loop)]
    for n in 0..ids.len() {
        let Some(drawing) = part.program[n].downcast_ref::<workpiece_ops_drawing::Drawing>() else {
            continue;
        };
        let Ok(world) = build_steps(n + 1) else {
            continue;
        };
        let mut sheet = drawing.args.clone();
        let name = std::path::Path::new(path)
            .file_stem()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let properties = part.properties.clone();
        let parts = workspace.placed_names(path);
        let densities: BTreeMap<String, f64> = workspace
            .library
            .materials
            .iter()
            .map(|m| (m.name.clone(), m.density))
            .collect();
        let subject = workpiece_ops_drawing::lay::Subject {
            world: &world,
            unit: part.unit,
            material: part.material.clone(),
            name: &name,
            properties: &properties,
            parts: &parts,
            densities: &densities,
        };
        let kept = workpiece_ops_drawing::lay::Kept::default();
        // What the recipe keeps off the sheet, by name.
        let mut kept_off: std::collections::BTreeSet<String> = Default::default();
        let mut placement_problems: Vec<String> = Vec::new();
        // The dimensions whose place the recipe says: those stay there.
        let mut held: std::collections::BTreeSet<String> = Default::default();
        if let Some(asked) = all_notes.get(&ids[n]) {
            // Everything said of the model that stands nowhere yet, put
            // where it would stand by itself, each once: in the view
            // asked for for what says it (a sketch, a dimension step),
            // or the first that shows it; nothing of what is to stay
            // off the sheet.
            let said_by = |note: &str| note.split('/').next().unwrap_or(note).to_owned();
            let hidden: Vec<&str> = asked["hide_dimensions_of"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let mut seen: std::collections::BTreeSet<String> =
                sheet.notes.iter().map(|n| n.note.clone()).collect();
            // (Each thing said of the model, with where it is asked to
            // stand and how many decimals it has.)
            let mut wanted: Vec<workpiece_ops_drawing::lay::Wanted> = Vec::new();
            let mut decimals: Vec<Option<u8>> = Vec::new();
            // (And of a hole's callout, how many holes it says it is of.)
            let mut counts: Vec<Option<u32>> = Vec::new();
            // (What the recipe says of a dimension by its own name — a
            // step's `bore/callout` — has to name one; a step, one of
            // the part's steps.)
            for key in [
                "dimension_views",
                "dimension_at",
                "dimension_decimals",
                "dimension_count",
            ] {
                for asked_of in asked[key].as_object().into_iter().flatten().map(|(k, _)| k) {
                    let known = match asked_of.contains('/') {
                        true => world.said(asked_of).is_some(),
                        false => ids.iter().any(|id| id == asked_of),
                    };
                    anyhow::ensure!(
                        known,
                        "the drawing {} is told where {asked_of:?} goes, but the model says nothing by that name \
                         (what it says: {})",
                        ids[n],
                        world
                            .annotations
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            // (And what a placed part says of itself that the recipe puts
            // on this sheet: `part/sketch/d3`.)
            let of_parts: Vec<String> = ["dimension_views", "dimension_at"]
                .iter()
                .flat_map(|key| {
                    asked[*key]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .map(|(k, _)| k.clone())
                })
                .filter(|k| !world.annotations.contains_key(k) && world.said(k).is_some())
                .collect();
            for said in world.annotations.keys().cloned().chain(of_parts) {
                let by = said_by(&said);
                // (What is said of this one dimension goes before what
                // is said of all its sketch's.)
                let own = names.values().find_map(|named| named.3.get(&said));
                // (A view asked for this one dimension puts it on the
                // sheet, though its sketch's are kept off.)
                let off = match own
                    .and_then(|o| o.get("on_drawing"))
                    .and_then(Value::as_bool)
                {
                    Some(on) => !on,
                    None if own.and_then(|o| o.get("show_in")).is_some() => false,
                    None => hidden.contains(&said.as_str()) || hidden.contains(&by.as_str()),
                };
                if off {
                    kept_off.insert(said.clone());
                    continue;
                }
                if !seen.insert(said.clone()) {
                    continue;
                }
                let view = own
                    .and_then(|o| o.get("show_in"))
                    .and_then(Value::as_str)
                    .or_else(|| asked["dimension_views"][&said].as_str())
                    .or_else(|| asked["dimension_views"][&by].as_str())
                    .map(str::to_owned);
                // Where its text is to stand, if the recipe says: from
                // the view's middle.
                let at = own
                    .and_then(|o| o.get("at"))
                    .or_else(|| asked["dimension_at"].get(&said))
                    .or_else(|| asked["dimension_at"].get(&by))
                    .and_then(|at| Some([at.get(0)?.as_f64()?, at.get(1)?.as_f64()?]));
                decimals.push(
                    own.and_then(|o| o.get("decimals"))
                        .or_else(|| asked["dimension_decimals"].get(&said))
                        .or_else(|| asked["dimension_decimals"].get(&by))
                        .and_then(Value::as_u64)
                        .map(|d| d as u8),
                );
                counts.push(
                    own.and_then(|o| o.get("count"))
                        .or_else(|| asked["dimension_count"].get(&said))
                        .or_else(|| asked["dimension_count"].get(&by))
                        .and_then(Value::as_u64)
                        .map(|n| n as u32),
                );
                wanted.push(workpiece_ops_drawing::lay::Wanted {
                    note: said,
                    view,
                    at,
                });
            }
            // Each put by itself in turn, clear of what stands before it
            // (in a view that was not asked for where the one asked for
            // does not see it true).
            let places = workpiece_ops_drawing::lay::place_all(&sheet, &subject, &kept, &wanted);
            for (((want, place), decimals), count) in
                wanted.into_iter().zip(places).zip(decimals).zip(counts)
            {
                // (In no view: named as missing below.)
                let Some((view, at, as_asked)) = place else {
                    continue;
                };
                if let (Some(asked), false) = (&want.view, as_asked) {
                    placement_problems.push(format!(
                        "the dimension {} cannot be shown true in the view {asked} (it would be seen askew or \
                         end on there, or it lies outside what a detail shows, or there is no such view): it is in \
                         the view {view} instead",
                        want.note
                    ));
                }
                if want.at.is_some() {
                    held.insert(want.note.clone());
                }
                sheet.notes.push(workpiece_ops_drawing::sheet::PlacedNote {
                    decimals,
                    note: want.note,
                    view,
                    at,
                    starts: [None; 2],
                    count,
                });
            }
            // What stands off the sheet's frame by itself is brought
            // in: as far as it is out, a few times over (bringing one
            // in can change where its lines run).
            let [w, h] = sheet.size();
            for _ in 0..4 {
                let laid = workpiece_ops_drawing::lay::lay_out(&sheet, &subject, &kept, None);
                let mut moved = false;
                for note in laid.notes.iter().filter(|n| {
                    n.kind == workpiece_ops_drawing::lay::Placing::Note && n.preview.is_none()
                }) {
                    let Some(placed) = sheet.notes.get_mut(note.index) else {
                        continue;
                    };
                    if held.contains(&placed.note) {
                        continue;
                    }
                    let [lo, hi] = note.rect;
                    let (edge, room) = (12.0, 2.0);
                    let dx = (edge - lo.x).max(0.0) - (hi.x - (w - edge)).max(0.0);
                    let dy = (edge - lo.y).max(0.0) - (hi.y - (h - edge)).max(0.0);
                    if dx != 0.0 || dy != 0.0 {
                        placed.at[0] += dx + room * dx.signum();
                        placed.at[1] += dy + room * dy.signum();
                        moved = true;
                    }
                }
                if !moved {
                    break;
                }
            }
            if let Some(drawing) = part.program[n].downcast_mut::<workpiece_ops_drawing::Drawing>()
            {
                drawing.args = sheet.clone();
                placed_any = true;
            }
        }
        let laid = workpiece_ops_drawing::lay::lay_out(&sheet, &subject, &kept, None);
        // What reaches off the sheet's frame: a view or a dimension's
        // text that would be cut off on paper.
        let mut laid = laid;
        let [w, h] = laid.size;
        let outside = |lo: workpiece_core::math::Point2, hi: workpiece_core::math::Point2| {
            lo.x < 10.0 || lo.y < 10.0 || hi.x > w - 10.0 || hi.y > h - 10.0
        };
        laid.problems.extend(placement_problems);
        // What is to be on the sheet and stands nowhere: named.
        let missing: Vec<&String> = laid
            .missing
            .iter()
            .filter(|name| !kept_off.contains(*name))
            .collect();
        for name in &missing {
            laid.problems.push(format!(
                "the dimension {name} is in no view: none of them sees it true (or it was asked for in a view there is none of)"
            ));
        }
        let missing = missing.len();
        for view in &laid.views {
            if outside(view.centre - view.half, view.centre + view.half) {
                laid.problems.push(format!(
                    "the view {} reaches off the sheet's frame: move it",
                    view.id
                ));
            }
        }
        let off = laid
            .notes
            .iter()
            .filter(|n| outside(n.rect[0], n.rect[1]))
            .count();
        if off > 0 {
            laid.problems.push(format!(
                "{off} dimension(s) stand off the sheet's frame: move their view inwards or show them in another view"
            ));
        }
        let sheet_text =
            inventory::of_sheet(&sheet, &laid, &world, names, &sketches, &step_args, &parts);
        let mut one = json!({
            "inventory": sheet_text,
            "id": ids[n],
            "views": laid.views.len(),
            "dimensions_shown": sheet.notes.len() + sheet.own.len(),
            "dimensions_missing": missing,
            "dimensions_kept_off": kept_off.len(),
            "problems": laid.problems,
        });
        if let Some(dir) = &render {
            std::fs::create_dir_all(dir)?;
            let (pdf, svg) = (
                format!("{dir}/{}.pdf", ids[n]),
                format!("{dir}/{}.svg", ids[n]),
            );
            std::fs::write(&pdf, laid.pdf())?;
            std::fs::write(&svg, laid.svg())?;
            one["pdf"] = json!(pdf);
            one["svg"] = json!(svg);
        }
        println!(
            "drawing {}: {} views, {} dimensions shown, {} missing",
            ids[n],
            laid.views.len(),
            sheet.notes.len() + sheet.own.len(),
            missing
        );
        for problem in &laid.problems {
            println!("        {problem}");
        }
        drawings.push(one);
    }
    if placed_any {
        std::fs::write(path, part.to_text()?)?;
    }
    let mut all = json!({ "part": path, "failed_steps": failed, "steps": steps, "lint": lint, "drawings": drawings });
    if let Some(world) = &last {
        timed("lint and drawings");
        all["bodies"] = bodies(world, workpiece_core::Pose::IDENTITY);
        // The parts placed in it, each with where it is, its bodies (their
        // faces and edges where they are in this part) and what it
        // weighs; and what it all weighs.
        let own = workspace.part(path)?;
        let cubic = cubic_metres(own.unit);
        let mut placed = Vec::new();
        placed_parts(
            world,
            path,
            &workspace,
            "",
            workpiece_core::Pose::IDENTITY,
            &mut placed,
        );
        let mut parts = Vec::new();
        let (volume, mass, material, density) = weighed(world, &workspace.library, cubic);
        if volume > 0.0 {
            parts.push(json!({
                "name": "(own bodies)", "file": path, "material": material,
                "volume": volume, "mass": mass, "density": density,
            }));
        }
        for p in &placed {
            parts.push(json!({
                "name": p["name"], "file": p["file"], "material": p["material"],
                "volume": p["volume"], "mass": p["mass"], "density": p["density"],
            }));
        }
        // What all of it weighs whose material is known, and what is not
        // weighed (said beside it, not taken as nothing).
        let known: Vec<f64> = parts.iter().filter_map(|p| p["mass"].as_f64()).collect();
        let total = (!known.is_empty()).then(|| known.iter().sum::<f64>());
        let unweighed: Vec<&str> = parts
            .iter()
            .filter(|p| p["mass"].is_null())
            .filter_map(|p| p["name"].as_str())
            .collect();
        for p in &placed {
            let at = &p["pose"]["at"];
            println!(
                "placed  {} ({}) at {at}, turned {}",
                p["name"].as_str().unwrap_or_default(),
                p["file"].as_str().unwrap_or_default(),
                p["pose"]["euler"],
            );
        }
        for p in &parts {
            match p["mass"].as_f64() {
                Some(kg) => println!(
                    "mass    {}: {} g ({}{})",
                    p["name"].as_str().unwrap_or_default(),
                    kg * 1000.0,
                    p["material"].as_str().unwrap_or_default(),
                    p["density"]
                        .as_f64()
                        .map(|d| format!(", {d:.6} kg/m³ from the workspace's material library"))
                        .unwrap_or_default()
                ),
                None => println!(
                    "mass    {}: not known: say what it is made of (a material of the library)",
                    p["name"].as_str().unwrap_or_default()
                ),
            }
        }
        if let (Some(total), true) = (total, parts.len() > 1) {
            match unweighed.is_empty() {
                true => println!("mass    in all: {} g", total * 1000.0),
                false => println!(
                    "mass    in all: at least {} g ({} not weighed: {})",
                    total * 1000.0,
                    unweighed.len(),
                    unweighed.join(", ")
                ),
            }
        }
        all["unweighed"] = json!(unweighed);
        all["placed"] = json!(placed);
        all["masses"] = json!(parts);
        all["mass"] = json!(total);
        timed("faces, edges and how the bodies are spread");
        if let Some((lo, hi)) = world.extent() {
            all["extent"] = json!([lo.to_array(), hi.to_array()]);
        }
        for (body, volume) in volumes(world) {
            println!("        {body}: volume {volume}");
        }
        // (`--no-pictures`: the sheets are drawn, the four pictures not.)
        if let (Some(dir), false) = (render, std::env::args().any(|a| a == "--no-pictures")) {
            std::fs::create_dir_all(&dir)?;
            all["pictures"] = json!(pictures(world, &workspace.library, &dir)?);
            timed("pictures");
        }
        // A picture traced as the Render step traces one, if asked for.
        if let Some(file) = arg("--picture") {
            let size = match arg("--size") {
                Some(size) => {
                    let (w, h) = size
                        .split_once(['x', 'X'])
                        .context("--size is WIDTHxHEIGHT, e.g. 1200x900")?;
                    [w.parse()?, h.parse()?]
                }
                None => [1200, 900],
            };
            let samples = arg("--samples").map_or(Ok(256), |n| n.parse())?;
            let seconds: f64 = arg("--seconds").map_or(Ok(5.0), |n| n.parse())?;
            let passes = traced(world, &workspace.library, &file, size, samples, seconds)?;
            println!(
                "picture {file}: {}×{}, {passes} samples a pixel",
                size[0], size[1]
            );
            all["picture"] = json!({ "file": file, "size": size, "samples": passes });
            timed("picture");
        }
    }
    println!("{failed} of {} steps failed", ids.len());
    for line in &lint {
        println!("lint    {line}");
    }
    if let Some(report) = report {
        std::fs::write(report, serde_json::to_string_pretty(&all)? + "\n")?;
    }
    Ok(())
}

/// The folder the part at `path` is in: where the parts it places and
/// its materials are.
fn folder_of(path: &str) -> std::path::PathBuf {
    std::path::Path::new(path)
        .parent()
        .map_or_else(|| ".".into(), std::path::Path::to_path_buf)
}

/// The materials of the folder `dir`: its `materials.json`, or those a
/// library starts with.
fn library_in(dir: &std::path::Path) -> anyhow::Result<workpiece_part::Library> {
    let file = dir.join(workpiece_part::LIBRARY_FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => workpiece_part::Library::from_text(&text)
            .with_context(|| format!("reading {}", file.display())),
        Err(_) => Ok(workpiece_part::Library::default()),
    }
}

/// A workspace of `part`, at `path`, and of every part it places (and
/// they place), each read from its file in the folder `part` is in;
/// with that folder's materials.
fn workspace_of(path: &str, part: Part) -> anyhow::Result<Workspace> {
    let dir = folder_of(path);
    let mut parts = BTreeMap::from([(path.to_owned(), part)]);
    let mut wanted: Vec<String> = vec![path.to_owned()];
    while let Some(next) = wanted.pop() {
        let placed: Vec<String> = parts[&next]
            .program
            .iter()
            .filter_map(|op| op.placed().map(|(placed, _)| placed.to_owned()))
            .collect();
        for placed in placed {
            if parts.contains_key(&placed) || workpiece_part::is_step(&placed) {
                continue;
            }
            let file = dir.join(&placed);
            let text = std::fs::read_to_string(&file).with_context(|| {
                format!(
                    "{next} places {placed}, which is no file beside it ({})",
                    file.display()
                )
            })?;
            let read = Part::from_text(&text).with_context(|| format!("reading {placed}"))?;
            parts.insert(placed.clone(), read);
            wanted.push(placed);
        }
    }
    let mut workspace = Workspace::new(parts);
    workspace.library = library_in(&dir)?;
    Ok(workspace)
}

/// A cubic unit of a part in `unit`, in cubic metres.
fn cubic_metres(unit: workpiece_core::Unit) -> f64 {
    (unit.millimetres() / 1000.0).powi(3)
}

/// What the solid bodies of `world` itself (not what is placed in it)
/// come to: their volume, their mass in kilograms by what each is made
/// of — the body's own material, else its part's — as `library` has
/// it (none where that is not known), and the names of those
/// materials. `cubic` is a cubic unit of the part, in cubic metres.
fn weighed(
    world: &World,
    library: &workpiece_part::Library,
    cubic: f64,
) -> (f64, Option<f64>, String, Option<f64>) {
    let topo = &world.topo;
    let (mut volume, mut mass, mut known) = (0.0, 0.0, true);
    let mut made_of = std::collections::BTreeSet::new();
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let (Some(name), Ok(solid)) = (topo.name_of(id), workpiece_boolean::read(topo, id)) else {
            continue;
        };
        let v = workpiece_boolean::measure(&solid).volume;
        let material = world.body_look(name).material;
        let density = material
            .as_deref()
            .and_then(|m| library.get(m))
            .map(|m| m.density);
        volume += v;
        match density {
            Some(d) => mass += v * cubic * d,
            None => known = false,
        }
        made_of.insert(material.unwrap_or_else(|| "(none said)".to_owned()));
    }
    let made_of = made_of.into_iter().collect::<Vec<_>>().join(", ");
    // (The density weighed by, kg/m³: the library's for the material
    // named, over all of the bodies.)
    let density = (known && volume > 0.0).then(|| mass / (volume * cubic));
    (volume, known.then_some(mass), made_of, density)
}

/// The parts placed in `world` — the built part at `path` of
/// `workspace` — and in them, in turn: each by where it is (`within`
/// names the placed parts it is in, `at` is where `world` is in the
/// part reported on), with the file it is of, its pose, material,
/// bodies (their faces and edges where they are in the part reported
/// on), volume and mass.
fn placed_parts(
    world: &World,
    path: &str,
    workspace: &Workspace,
    within: &str,
    at: workpiece_core::Pose,
    out: &mut Vec<Value>,
) {
    let program = workspace
        .part(path)
        .map(|p| p.program.as_slice())
        .unwrap_or_default();
    let unit = workspace.part(path).map(|p| p.unit).unwrap_or_default();
    for (name, instance) in &world.instances {
        // The step that placed it: called as it is, or what it is one of
        // (`pattern/2`).
        let placed = program.iter().find_map(|op| {
            let id = op.id();
            let of = name == id || name.strip_prefix(id).is_some_and(|r| r.starts_with('/'));
            op.placed().filter(|_| of)
        });
        let (file, variant) = placed.map_or((String::new(), None), |(f, v)| {
            (f.to_owned(), v.map(str::to_owned))
        });
        let full = if within.is_empty() {
            name.clone()
        } else {
            format!("{within}/{name}")
        };
        let pose = at * instance.pose;
        let unit = workspace.part(&file).map_or(unit, |p| p.unit);
        let (volume, mass, material, density) =
            weighed(&instance.world, &workspace.library, cubic_metres(unit));
        let rotation = pose.rotation();
        out.push(json!({
            "name": full,
            "file": file,
            "variant": variant,
            "pose": {
                "at": pose.translation().to_array(),
                "euler": pose.euler_degrees(),
                "rotation": [rotation.x, rotation.y, rotation.z, rotation.w],
            },
            "fixed": instance.fixed,
            "material": material,
            "volume": volume,
            "mass": mass,
            "density": density,
            "bodies": bodies(&instance.world, pose),
        }));
        placed_parts(&instance.world, &file, workspace, &full, pose, out);
    }
}

/// The solid bodies of `world` with their volumes.
fn volumes(world: &World) -> BTreeMap<String, f64> {
    let topo = &world.topo;
    let mut all = BTreeMap::new();
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        if let (Some(name), Ok(solid)) = (topo.name_of(id), workpiece_boolean::read(topo, id)) {
            all.insert(name.to_owned(), workpiece_boolean::measure(&solid).volume);
        }
    }
    all
}

/// The solid bodies of `world`: how each is spread, and its faces and
/// edges by name with where they are, to pick them by — where `pose`
/// puts them.
fn bodies(world: &World, pose: workpiece_core::Pose) -> Value {
    let topo = &world.topo;
    let mut all = Vec::new();
    let array = |p: Point3| {
        let p = pose.transform_point(p);
        json!([p.x, p.y, p.z])
    };
    let direction = |v: Point3| {
        let v = pose.transform_vector(v);
        json!([v.x, v.y, v.z])
    };
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let (Some(name), Ok(solid)) = (topo.name_of(id), workpiece_boolean::read(topo, id)) else {
            continue;
        };
        // (Not how its volume is spread: its moments take a fine mesh of
        // every face, far longer than all the rest.)
        let spread = workpiece_boolean::measure(&solid);
        let faces: Vec<Value> = (0..solid.faces.len())
            .filter_map(|f| {
                let face = &solid.faces[f];
                let (on, out) = workpiece_shape::on::middle(&solid, f);
                // (What kind of surface it is, however it was made.)
                use workpiece_shape::known::{Known, known};
                let kind = match known(&face.surface) {
                    Some(Known::Plane { .. }) => "plane",
                    Some(Known::Cylinder { .. }) => "cylinder",
                    Some(Known::Cone { .. }) => "cone",
                    None => "other",
                };
                Some(json!({
                    "name": face.name.as_ref()?,
                    "area": workpiece_boolean::face_area(&solid, f),
                    "middle": array(on),
                    "normal": direction(out),
                    "flat": kind == "plane",
                    "surface": kind,
                }))
            })
            .collect();
        let edges: Vec<Value> = solid
            .edges
            .iter()
            .filter_map(|edge| {
                let (a, b) = edge.range;
                let at = |k: usize| edge.point(a + (b - a) * k as f64 / 16.0);
                let length: f64 = (0..16).map(|k| at(k).distance(at(k + 1))).sum();
                Some(json!({
                    "name": edge.name.as_ref()?,
                    "from": array(at(0)),
                    "to": array(at(16)),
                    "middle": array(at(8)),
                    "length": length,
                    "straight": edge.curve.degree() == 1,
                }))
            })
            .collect();
        all.push(json!({
            "name": name,
            "volume": spread.volume,
            "centre": array(spread.centre),
            "faces": faces,
            "edges": edges,
        }));
    }
    json!(all)
}

/// Pictures of `world` from four sides, written into `dir`: their
/// paths.
/// The value given after `name` on the command line, if it is given.
fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

/// Traces a picture of `world` (made of what `library` has) into the
/// PNG file `file`: `size` pixels, all of the model in view from the
/// front right and above, on a floor that catches its shadow; as many
/// as `samples` rays a pixel, as long as `seconds` allow (one at
/// least). How many each pixel got.
fn traced(
    world: &World,
    library: &workpiece_part::Library,
    file: &str,
    size: [usize; 2],
    samples: usize,
    seconds: f64,
) -> anyhow::Result<usize> {
    let taking = workpiece_ui::preview::Taking {
        ground: true,
        ..workpiece_ui::preview::Taking::new(size, samples)
    };
    let started = std::time::Instant::now();
    let passes = std::cell::Cell::new(1);
    let more = || {
        let more = started.elapsed().as_secs_f64() < seconds;
        passes.set(passes.get() + usize::from(more));
        more
    };
    let rgba = workpiece_ui::preview::shot(world, library, &taking, &more)
        .context("there is no body to take a picture of")?;
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    if let Some(dir) = std::path::Path::new(file).parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let mut encoder =
        png::Encoder::new(std::fs::File::create(file)?, size[0] as u32, size[1] as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder.write_header()?.write_image_data(&rgb)?;
    Ok(passes.get().min(samples.max(1)))
}

fn pictures(
    world: &World,
    library: &workpiece_part::Library,
    dir: &str,
) -> anyhow::Result<Vec<String>> {
    use glam::Vec3;
    let (width, height) = (640, 480);
    let views = [
        ("iso", None),
        ("front", Some((Vec3::NEG_Y, Vec3::Z))),
        ("top", Some((Vec3::Z, Vec3::Y))),
        ("right", Some((Vec3::X, Vec3::Z))),
    ];
    // (The four at once, each on a thread of its own.)
    let shots: Vec<Option<Vec<u8>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = views
            .iter()
            .map(|(_, from)| {
                let from = *from;
                scope.spawn(move || {
                    let taking = workpiece_ui::preview::Taking {
                        from,
                        ..workpiece_ui::preview::Taking::new([width, height], 48)
                    };
                    workpiece_ui::preview::shot(world, library, &taking, &|| true)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .collect()
    });
    let mut written = Vec::new();
    for ((name, _), shot) in views.into_iter().zip(shots) {
        let Some(rgba) = shot else {
            continue;
        };
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let path = format!("{dir}/{name}.png");
        let mut encoder =
            png::Encoder::new(std::fs::File::create(&path)?, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgb)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_with_an_argument_its_kind_lacks_is_refused() {
        let recipe = |args: Value| json!({ "steps": [{ "op": "Hole", "id": "h", "args": args }] });
        // A known argument is let by (and so is an empty unknown one).
        assert!(
            made(
                &recipe(json!({ "diameter": { "value": 5.0 } })),
                "part.workpiece"
            )
            .is_ok()
        );
        let said = match made(&recipe(json!({ "next": true })), "part.workpiece") {
            Err(e) => e.to_string(),
            Ok(_) => panic!("refused"),
        };
        assert!(
            said.contains("step 0 (Hole h)") && said.contains("\"next\""),
            "{said}"
        );
        assert!(
            said.contains("extent"),
            "it names the valid arguments: {said}"
        );
    }

    #[test]
    fn a_plane_is_said_as_every_step_says_a_reference() {
        // The origin's planes by their names; a datum and a face as any
        // step refers to them; the sketch-only forms of old are refused.
        assert!(plane(&json!("XY")).is_ok());
        assert_eq!(
            plane(&json!({ "Datum": "mid" })).unwrap(),
            Reference::datum("mid")
        );
        assert_eq!(
            plane(&json!({ "Topology": "base/end" })).unwrap(),
            Reference::topology("base/end")
        );
        assert!(plane(&json!({ "datum": "mid" })).is_err());
    }

    #[test]
    fn a_sketch_of_an_assembly_projects_an_edge_of_a_placed_part() {
        let folder =
            std::env::temp_dir().join(format!("workpiece_cli_placed_{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let in_folder = |name: &str| folder.join(name).to_string_lossy().into_owned();
        let rect = |id: &str, w: f64, h: f64| {
            json!({ "id": id, "plane": "XY",
                "points": { "a": [0.0, 0.0], "b": [w, 0.0], "c": [w, h], "d": [0.0, h] },
                "lines": { "ab": ["a", "b"], "bc": ["b", "c"], "cd": ["c", "d"], "da": ["d", "a"] },
                "constraints": [["horizontal", "ab"], ["horizontal", "cd"],
                                ["vertical", "bc"], ["vertical", "da"]] })
        };
        let plate = json!({ "material": null, "steps": [
            { "sketch": rect("s", 40.0, 20.0) },
            { "op": "Extrude", "id": "plate",
              "args": { "sketch": { "Profile": "s" }, "distance": 10.0, "combine": "NewBody" } },
        ] });
        let (part, _) = made(&plate, &in_folder("plate.workpiece")).unwrap();
        std::fs::write(in_folder("plate.workpiece"), part.to_text().unwrap()).unwrap();
        // The assembly places it, projects the plate's top edge into a
        // sketch, lays a square on it and extrudes that.
        let mut square = rect("q", 10.0, 10.0);
        square["constraints"]
            .as_array_mut()
            .unwrap()
            .push(json!(["point_on", "a", "rim"]));
        square["project"] = json!({ "rim": { "Placed": { "instance": "p1",
            "source": { "Topology": "plate/end/e11" } } } });
        let assembly = json!({ "steps": [
            { "op": "AddPart", "id": "p1",
              "args": { "part": "plate.workpiece", "hold": { "constraint": "Fixed", "flip": false }, "bodies": [] },
              "pose": { "at": [0.0, 0.0, 5.0], "euler": [0.0, 0.0, 0.0] } },
            { "sketch": square },
            { "op": "Extrude", "id": "x",
              "args": { "sketch": { "Profile": "q" }, "distance": 3.0, "combine": "NewBody" } },
        ] });
        let made_in_folder = made(&assembly, &in_folder("asm.workpiece"));
        let (assembly, _) = match made_in_folder {
            Ok(made) => made,
            Err(e) => panic!("{e:#}"),
        };
        let workspace = workspace_of(&in_folder("asm.workpiece"), assembly).unwrap();
        let built = workpiece_part::Engine::inline()
            .build(
                &std::sync::Arc::new(workspace),
                &in_folder("asm.workpiece"),
                None,
                &|_| true,
                &workpiece_part::Cancel::new(),
            )
            .unwrap();
        assert!(built.world.extent().is_some());
        std::fs::remove_dir_all(&folder).ok();
    }
}
