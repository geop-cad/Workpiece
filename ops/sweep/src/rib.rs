//! Rib: a thin wall from the curves of a sketch to the body beside
//! them.

use anyhow::{Context, ensure};
use shapecraft_boolean::{Op, combine, read};
use shapecraft_core::math::Point3;
use shapecraft_core::world::topo::Curve3;
use shapecraft_core::{Builder, Measure, Operation};

use shapecraft_shape::bodies;
use shapecraft_shape::sweep::Frame;
use shapecraft_shape::swept::along_path;

/// The rib's top, how thick it is, and what it stiffens.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, shapecraft_core::Arguments,
)]
pub struct RibArgs {
    /// The sketch whose curves, one after the other, are the rib's free
    /// edge: the rib lies in the sketch's plane.
    pub sketch: Option<shapecraft_core::Reference<shapecraft_core::Sketched>>,
    /// How thick the rib is: half to either side of the sketch's plane.
    pub thickness: Measure,
    /// Whether it grows to the other side of its curves.
    #[serde(default)]
    #[arg(flip, row)]
    pub flip: bool,
    /// The body it stiffens; `None` for every solid body.
    #[serde(default)]
    #[arg(label = "Body", empty = "All bodies")]
    pub target: Option<shapecraft_core::Reference<shapecraft_core::world::topo::BodyId>>,
}

impl Default for RibArgs {
    fn default() -> Self {
        Self {
            sketch: None,
            thickness: Measure::new(1.0),
            flip: false,
            target: None,
        }
    }
}

/// Fills in a wall between the curves of a sketch and the body: from
/// the curves, in the sketch's plane and square to the line between
/// their ends, as far as the body lets it, and joined to the body.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rib {
    /// The operation's id.
    pub id: String,
    /// The curves, the thickness and the body.
    pub args: RibArgs,
}

impl Operation for Rib {
    type Args = RibArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &RibArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut RibArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = shapecraft_shape::bodies::Out::default();
        let args = &self.args;
        let thickness = (*builder).measure(&args.thickness, "the thickness")?;
        ensure!(thickness > 0.0, "a rib has some thickness");
        let world = builder.tracked();
        let sketch = args.sketch.as_ref().context("pick a sketch")?;
        let runs = runs(world, sketch)?;
        let topo = &world.topo;
        let targets = bodies::targets(topo, &self.id, &args.target)?;
        ensure!(!targets.is_empty(), "there is no solid body to stiffen");
        let all: Vec<_> = targets.iter().map(|t| t.0).collect();
        // The bodies as each rib leaves them: a rib is made of what the
        // ones before it leave, and joined to them, as separate steps
        // would.
        let mut joined: Vec<(
            Option<shapecraft_core::world::topo::BodyId>,
            String,
            shapecraft_boolean::Solid,
        )> = Vec::new();
        for (body, name) in &targets {
            joined.push((Some(*body), name.clone(), read(topo, *body)?));
        }
        // One rib to a run of curves; runs that cross or touch grow
        // into one web.
        for (curves, normal) in &runs {
            let curves: &[Curve3] = curves;
            let normal = *normal;
            let end = |curve: &Curve3, last: bool| {
                let (a, b) = curve.domain();
                curve.point_at(if last { b } else { a })
            };
            let (first, last) = (end(&curves[0], false), end(&curves[curves.len() - 1], true));
            let across = normal
                .cross(last - first)
                .try_normalize()
                .context("the rib's curves end where they start")?;
            // The way it is told to grow; or, if nothing stops it there,
            // the other.
            let grown = |way: Point3| -> anyhow::Result<shapecraft_boolean::Solid> {
                // (From the point of the curves that stands farthest back
                // from the body: a bent chain's middle may, not only its
                // ends.)
                let behind = curves
                    .iter()
                    .flat_map(|curve| {
                        let (a, b) = curve.domain();
                        (0..=32).map(move |k| curve.point_at(a + (b - a) * f64::from(k) / 32.0))
                    })
                    .fold(
                        first,
                        |back, p| if p.dot(way) < back.dot(way) { p } else { back },
                    );
                let far = bodies::reach(topo, &all, behind, way)
                    .filter(|far| *far > 1e-9)
                    .context("no body lies that way")?;
                let far = far * 1.1 + thickness;
                // The outline in the sketch's plane: the curves, and themselves
                // again that far on, joined at their ends. Half the thickness
                // behind the plane, to be carried through it.
                let back = -normal * (thickness / 2.0);
                let moved = |curve: &Curve3, by: Point3| {
                    let points = curve.control_points().iter().map(|p| *p + by).collect();
                    Curve3::new(
                        curve.degree(),
                        curve.knots().to_vec(),
                        points,
                        Some(curve.weights().to_vec()),
                    )
                };
                let mut outline = Vec::new();
                for curve in curves {
                    outline.push(moved(curve, back)?);
                }
                outline.push(Curve3::line(last + back, last + back + way * far)?);
                for curve in curves.iter().rev() {
                    outline.push(moved(curve, back + way * far)?.reversed());
                }
                outline.push(Curve3::line(first + back + way * far, first + back)?);
                let through = [Curve3::line(first + back, first - back)?];
                let wall = along_path(&self.id, &outline, &through, false, way, 0.0, 1.0)?;
                // Of what the body leaves of it, the piece at the curves.
                let mut rest = wall;
                for (.., solid) in &joined {
                    rest = combine(Op::Cut, &rest, solid)
                        .context("fitting the rib to the body")?
                        .solid;
                }
                let near = thickness * 1e-3 + 1e-9;
                let at =
                    |p: Point3| p.distance(first + back) < near || p.distance(first - back) < near;
                let rib = rest
                    .lumps()?
                    .into_iter()
                    .find(|lump| lump.vertices.iter().any(|v| at(v.point)))
                    .context("the rib's curves start inside the body")?;
                // (What reaches the far end of the wall met nothing on its way.)
                let beyond = first + way * far;
                let open = |p: Point3| {
                    p.distance(beyond + back) < near || p.distance(beyond - back) < near
                };
                ensure!(
                    !rib.vertices.iter().any(|v| open(v.point)),
                    "nothing lies either way to stop the rib"
                );
                Ok(rib)
            };
            let way = if args.flip { -across } else { across };
            let rib = grown(way).or_else(|first| grown(-way).map_err(|_| first))?;
            let mut all = rib;
            let mut name = None;
            let mut k = 0;
            while k < joined.len() {
                let outcome = combine(Op::Union, &joined[k].2, &all)
                    .with_context(|| format!("joining {}", joined[k].1))?;
                if outcome.touched {
                    let (body, target, _) = joined.remove(k);
                    if let Some(body) = body {
                        out.remove(topo, body);
                    }
                    all = outcome.solid;
                    name.get_or_insert(target);
                } else {
                    k += 1;
                }
            }
            joined.insert(0, (None, name.unwrap_or_else(|| self.id.clone()), all));
        }
        for (body, name, solid) in &joined {
            // (What nothing touched stays as it is.)
            if body.is_none() {
                out.write(solid, name);
            }
        }
        // Its thickness is a size of the model: said across it, at the
        // middle of its first curve.
        let (curves, normal) = &runs[0];
        let (a, b) = curves[0].domain();
        let middle = curves[0].point_at((a + b) / 2.0);
        let measured = shapecraft_core::world::Measured::Length {
            from: middle - normal * (thickness / 2.0),
            to: middle + normal * (thickness / 2.0),
            along: None,
        };
        let held =
            shapecraft_core::world::Annotation::held(measured, thickness, &args.thickness, 0.0);
        out.made
            .annotations
            .push((format!("{}/thickness", self.id), held));
        Ok(out.made)
    }

    fn summary(&self) -> String {
        format!("{} thick", self.args.thickness)
    }
}

/// The runs of curves of the sketch `source`, each with the normal of
/// its plane: curves that follow one another, end to end. At a point
/// where several meet — two that cross, one that ends on another — a
/// curve goes on into the one that carries on its way (within 25°); the
/// rest end there. A curve in space is one run.
fn runs(
    world: &shapecraft_core::World,
    source: &shapecraft_core::Reference<shapecraft_core::Sketched>,
) -> anyhow::Result<Vec<(Vec<Curve3>, Point3)>> {
    let name = source.name();
    let Some(profile) = world.profiles.get(name) else {
        let (curves, closes, normal) = shapecraft_shape::paths::path(world, source, Point3::ZERO)?;
        ensure!(!closes, "a rib's curves have two ends, on the body");
        return Ok(vec![(curves, normal)]);
    };
    let frame = Frame::of(profile);
    let edges = &profile.edges;
    ensure!(
        !edges.is_empty(),
        "the sketch {name:?} has no curve to make a rib of"
    );
    // An end of a curve: which, and which of its two ends.
    type End = (usize, bool);
    let forward = (0..edges.len())
        .map(|e| shapecraft_shape::paths::curve(profile, &frame, (e, false)))
        .collect::<anyhow::Result<Vec<_>>>()?;
    // Where the curve leaves from that end.
    let leaving = |(e, last): End| {
        let (from, to) = forward[e].domain();
        let way = if last {
            -forward[e].derivative_at(to).1
        } else {
            forward[e].derivative_at(from).1
        };
        way.normalize_or_zero()
    };
    let mut at_vertex = vec![Vec::new(); profile.vertices.len()];
    for e in 0..edges.len() {
        at_vertex[edges[e].start].push((e, false));
        at_vertex[edges[e].end].push((e, true));
    }
    // Which end each end goes on into.
    let mut partner: std::collections::HashMap<End, End> = std::collections::HashMap::new();
    for ends in &at_vertex {
        let mut free = ends.clone();
        while free.len() >= 2 {
            let mut best = None;
            for (i, a) in free.iter().enumerate() {
                for (j, b) in free.iter().enumerate().skip(i + 1) {
                    let straight = leaving(*a).dot(leaving(*b));
                    // (Two curves at a point always go on into each other.)
                    let wanted = if ends.len() == 2 { 2.0 } else { -0.9 };
                    if straight < wanted && best.is_none_or(|(_, _, s)| straight < s) {
                        best = Some((i, j, straight));
                    }
                }
            }
            let Some((i, j, _)) = best else { break };
            let (a, b) = (free[i], free[j]);
            partner.insert(a, b);
            partner.insert(b, a);
            free.remove(j);
            free.remove(i);
        }
    }
    let mut used = vec![false; edges.len()];
    let mut runs = Vec::new();
    for start in 0..edges.len() {
        // From an end that goes on into nothing.
        let Some(begin) = [(start, false), (start, true)]
            .into_iter()
            .find(|end| !partner.contains_key(end))
        else {
            continue;
        };
        if used[start] {
            continue;
        }
        let mut curves = Vec::new();
        let mut end = begin;
        loop {
            let (e, from_last) = end;
            used[e] = true;
            curves.push(shapecraft_shape::paths::curve(
                profile,
                &frame,
                (e, from_last),
            )?);
            match partner.get(&(e, !from_last)) {
                Some(next) => end = *next,
                None => break,
            }
        }
        runs.push((curves, frame.normal()));
    }
    ensure!(
        used.iter().all(|u| *u),
        "a rib's curves have two ends, on the body: a closed outline is no rib"
    );
    Ok(runs)
}
