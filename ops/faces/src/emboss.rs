//! Emboss and engrave: the areas of a sketch raised from faces of a
//! body, or sunk into them, following the faces.

use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use workpiece_boolean::{Op, Solid, combine, read};
use workpiece_core::math::Point3;
use workpiece_core::widgets::VBox;
use workpiece_core::world::topo::Topo;
use workpiece_core::world::{AreaPick, Profile};
use workpiece_core::{Builder, Edit, EditContext, EditVisualization, Measure, Operation, Widget};

use workpiece_shape::sweep::{self, Frame, Make};
use workpiece_shape::ui;

/// What is raised or sunk: the areas of a sketch thrown onto the faces,
/// or closed curves that lie on them already (a sketch projected onto
/// them, edges of the faces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Outline {
    /// The areas of a sketch, thrown onto the faces along its normal.
    #[default]
    Sketch,
    /// Closed curves on the faces: edges, or curves in space (wire
    /// bodies) such as a projection makes.
    Curves,
}

impl workpiece_core::Choices for Outline {
    const ALL: &'static [Self] = &[Outline::Sketch, Outline::Curves];

    fn label(self) -> &'static str {
        match self {
            Outline::Sketch => "Sketch",
            Outline::Curves => "Curves on face",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Outline::Sketch => "emboss-sketch",
            Outline::Curves => "emboss-curves",
        })
    }
}

/// What is raised or sunk, on which faces, and by how much.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, workpiece_core::Arguments,
)]
pub struct EmbossArgs {
    /// Whether the areas of a sketch are raised, or what closed curves
    /// on the faces bound.
    #[serde(default)]
    #[arg(choice, label = "")]
    pub outline: Outline,
    /// The sketch: its areas are thrown onto the faces along its
    /// plane's normal.
    #[arg(with = "sketch_fields", when = "self.outline == Outline::Sketch")]
    pub sketch: Option<workpiece_core::Reference<workpiece_core::Sketched>>,
    /// The areas of it, each by a point inside it or by its boundary;
    /// none for the whole sketch.
    #[serde(default)]
    pub areas: Vec<AreaPick>,
    /// The closed curves on the faces: edges, or curves in space (wire
    /// bodies, all their curves). A curve inside another makes a hole
    /// in what that one bounds.
    #[serde(default)]
    #[arg(with = "curves_field", when = "self.outline == Outline::Curves")]
    pub curves: Vec<workpiece_core::Reference<workpiece_core::Anything>>,
    /// The faces of the body they are raised from or sunk into. Where
    /// the sketch's areas are thrown onto them twice (both sides of a
    /// ring), only the first they meet takes them.
    #[arg(required)]
    pub faces: Vec<workpiece_core::Reference<workpiece_core::world::topo::FaceId>>,
    /// Whether the sides of what is raised (or sunk) run along the
    /// sketch's normal, or square to the face. (Of curves on the faces,
    /// always square to the face.)
    #[serde(default)]
    #[arg(
        label = "Sides",
        options = "Sketch normal|Face normal",
        icons = "emboss-along-sketch|emboss-along-face",
        when = "self.outline == Outline::Sketch"
    )]
    pub normal_to_face: bool,
    /// Whether they are sunk into the faces, not raised from them.
    #[serde(default)]
    #[arg(
        label = "Way",
        options = "Emboss (raised)|Engrave (sunk)",
        icons = "emboss-raised|emboss-sunk"
    )]
    pub engrave: bool,
    /// How high they stand, or how deep they are sunk.
    pub height: Measure,
}

impl Default for EmbossArgs {
    fn default() -> Self {
        Self {
            outline: Outline::Sketch,
            sketch: None,
            areas: Vec::new(),
            curves: Vec::new(),
            faces: Vec::new(),
            normal_to_face: false,
            height: Measure::new(0.5),
            engrave: false,
        }
    }
}

/// Raises the areas of a sketch — or what closed curves on the faces
/// bound — from faces of a body, or sinks them into the faces: each
/// area's top (or floor) follows the face, the height away from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Emboss {
    /// The operation's id.
    pub id: String,
    /// The areas, the faces and the height.
    pub args: EmbossArgs,
}

/// One area to raise: a region of a profile, thrown onto the faces along
/// the profile's normal; and where it is first met from.
pub struct Area<'a> {
    /// The profile.
    pub profile: &'a Profile,
    /// The region of it.
    pub region: usize,
    /// What it is called: its faces are named under it.
    pub name: String,
    /// The way the area is thrown onto the faces, from the profile's
    /// plane: of what it meets, what it meets first that way is taken.
    pub toward: Point3,
}

/// The areas of `profile`'s regions `regions`, thrown onto `solid`'s
/// faces `faces` from the profile's plane towards them (the plane's
/// normal, or against it where the faces are behind the plane).
pub fn sketch_areas<'a>(
    id: &str,
    solid: &Solid,
    faces: &[usize],
    profile: &'a Profile,
    regions: &[usize],
) -> Vec<Area<'a>> {
    let frame = Frame::of(profile);
    let (origin, normal) = (frame.origin, frame.normal());
    // Where the faces are, by their control points: in front of the
    // plane or behind it. (Faces all around it — a ring the plane runs
    // through — are taken in front.)
    let points: Vec<Point3> = faces
        .iter()
        .flat_map(|&f| solid.faces[f].surface.control_points().iter().copied())
        .collect();
    let middle = points.iter().copied().sum::<Point3>() / points.len().max(1) as f64;
    let size = points
        .iter()
        .map(|p| p.distance(middle))
        .fold(0.0_f64, f64::max);
    let ahead = (middle - origin).dot(normal);
    let toward = if ahead < -1e-6 * size.max(1.0) {
        -normal
    } else {
        normal
    };
    regions
        .iter()
        .map(|&region| Area {
            profile,
            region,
            name: format!("{id}/a{region}"),
            toward,
        })
        .collect()
}

/// `solid` with `areas` raised by `height` from its faces `faces` — or,
/// with less than nothing, sunk that deep into them. Each area takes
/// what of the faces it meets first; its sides run along the profile's
/// normal, or (`square`) square to the faces.
pub fn embossed(
    solid: &Solid,
    faces: &[usize],
    areas: &[Area<'_>],
    height: f64,
    square: bool,
) -> anyhow::Result<Solid> {
    ensure!(height != 0.0 && height.is_finite(), "by some height");
    // All within the height of the faces, on the side it goes to.
    let slab = workpiece_shape::thick::thicken(solid, faces, height, false, true)
        .context("following the faces")?;
    let mut made = solid.clone();
    for area in areas {
        let frame = Frame::of(area.profile);
        let origin = frame.origin;
        let mut far = 0.0_f64;
        for face in &solid.faces {
            for p in face.surface.control_points() {
                far = far.max(p.distance(origin));
            }
        }
        let far = far * 1.5 + height.abs() + 1.0;
        let walks = sweep::outline(area.profile, &[area.region])?;
        let mut scratch = Topo::new();
        let through = sweep::extrude(
            &mut scratch,
            &area.name,
            area.profile,
            &walks,
            true,
            (-far, far),
            Make::Solid,
        )?;
        let through = read(&scratch, through)?;
        let under = combine(Op::Intersect, &slab, &through)
            .context("throwing an area onto the faces")?
            .solid;
        let pieces = first_met(under.lumps()?, origin, area.toward);
        ensure!(!pieces.is_empty(), "an area is not over the faces");
        let op = if height > 0.0 { Op::Union } else { Op::Cut };
        for piece in pieces {
            let piece = if square {
                square_to_faces(&piece, height.abs(), &area.name)?
            } else {
                piece
            };
            made = combine(op, &made, &piece)
                .context("putting an area on the body")?
                .solid;
        }
    }
    Ok(made)
}

/// Of `lumps`, those met first going from `origin` along `toward`: the
/// nearest one ahead (or around `origin`), and those as near as it
/// reaches — not those behind it, on the far side of the body.
fn first_met(lumps: Vec<Solid>, origin: Point3, toward: Point3) -> Vec<Solid> {
    let span = |lump: &Solid| {
        lump.vertices
            .iter()
            .map(|v| (v.point - origin).dot(toward))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| {
                (lo.min(d), hi.max(d))
            })
    };
    let ahead: Vec<(Solid, (f64, f64))> = lumps
        .into_iter()
        .map(|l| {
            let s = span(&l);
            (l, s)
        })
        .filter(|(_, (_, hi))| *hi > 0.0)
        .collect();
    let Some(first) = ahead
        .iter()
        .map(|(_, s)| *s)
        .min_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return Vec::new();
    };
    ahead
        .into_iter()
        .filter(|(_, (lo, _))| *lo <= first.1)
        .map(|(l, _)| l)
        .collect()
}

/// What `piece` (an area thrown onto faces, within `height` of them)
/// stands on, raised from the faces by `height` with its sides square to
/// them: the faces of it that lie in the faces it stands on, thickened
/// (away from the body for what is raised, into it for what is sunk).
/// Its faces are named under `name`.
fn square_to_faces(piece: &Solid, height: f64, name: &str) -> anyhow::Result<Solid> {
    let on: Vec<usize> = (0..piece.faces.len())
        .filter(|&f| {
            piece.faces[f]
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("thick/") && n.contains("/near"))
        })
        .collect();
    ensure!(!on.is_empty(), "an area has no foot on the faces");
    let foot = workpiece_shape::surface::faces_of(piece, &on);
    let all: Vec<usize> = (0..foot.faces.len()).collect();
    // (The foot's faces look out of the piece, into what it stands on:
    // into the piece is less than nothing.)
    let mut made = workpiece_shape::thick::thicken(&foot, &all, -height, false, true)
        .context("raising an area square to the faces")?;
    for face in &mut made.faces {
        if let Some(rest) = face.name.as_deref().and_then(|n| n.strip_prefix("thick/")) {
            face.name = Some(format!("{name}/{rest}"));
        }
    }
    Ok(made)
}

/// The closed curves `curves` (edges, wire bodies) of `topo`, as areas:
/// each outer ring of them with the rings inside it, flattened along its
/// own normal onto the plane through its middle. Returns the profiles;
/// the areas are their only regions.
pub fn curve_profiles(topo: &Topo, curves: &[&str]) -> anyhow::Result<Vec<Profile>> {
    use workpiece_core::world::topo::{BodyId, BodyKind, Curve3, EdgeId};
    let mut pieces: Vec<(Curve3, f64, f64)> = Vec::new();
    for &name in curves {
        let edges: Vec<EdgeId> = match topo.resolve::<BodyId>(name).and_then(|b| topo.body(b)) {
            Some(body) if body.kind == BodyKind::Wire => body.free_edges.clone(),
            Some(_) => anyhow::bail!("{name:?} is a body, but no curve: pick its edges"),
            None => vec![
                topo.resolve(name)
                    .with_context(|| format!("there is no edge or curve {name:?}"))?,
            ],
        };
        for id in edges {
            let edge = topo.edge(id).context("an edge is gone")?;
            let line = topo.curve(edge.curve).context("an edge without a curve")?;
            pieces.push((line.clone(), edge.t0, edge.t1));
        }
    }
    ensure!(!pieces.is_empty(), "pick the curves that bound the areas");
    let ends: Vec<[Point3; 2]> = pieces
        .iter()
        .map(|(c, t0, t1)| [c.point_at(*t0), c.point_at(*t1)])
        .collect();
    let size = ends
        .iter()
        .flatten()
        .map(|p| p.distance(ends[0][0]))
        .fold(0.0_f64, f64::max);
    let near = (size * 1e-6).max(1e-9);
    // The rings: pieces end to end, each with the way it is walked.
    let mut left: Vec<usize> = (0..pieces.len()).collect();
    let mut rings: Vec<Vec<(usize, bool)>> = Vec::new();
    while let Some(first) = left.first().copied() {
        left.retain(|&k| k != first);
        let mut ring = vec![(first, false)];
        let start = ends[first][0];
        let mut at = ends[first][1];
        while at.distance(start) > near {
            let next = left.iter().copied().find_map(|k| {
                if ends[k][0].distance(at) <= near {
                    Some((k, false))
                } else if ends[k][1].distance(at) <= near {
                    Some((k, true))
                } else {
                    None
                }
            });
            let Some((k, reversed)) = next else {
                anyhow::bail!("the curves do not close around an area");
            };
            left.retain(|&j| j != k);
            ring.push((k, reversed));
            at = ends[k][if reversed { 0 } else { 1 }];
        }
        rings.push(ring);
    }
    // Each ring's points, its normal (by its area) and middle.
    let points = |ring: &[(usize, bool)]| -> Vec<Point3> {
        let mut out = Vec::new();
        for &(k, reversed) in ring {
            let (c, t0, t1) = &pieces[k];
            let mut line = c.polyline(*t0, *t1);
            if reversed {
                line.reverse();
            }
            line.pop();
            out.extend(line);
        }
        out
    };
    struct Ring {
        uses: Vec<(usize, bool)>,
        points: Vec<Point3>,
        normal: Point3,
        middle: Point3,
        area: f64,
    }
    let mut all: Vec<Ring> = rings
        .into_iter()
        .map(|uses| {
            let points = points(&uses);
            let mut normal = Point3::ZERO;
            for (k, p) in points.iter().enumerate() {
                normal += p.cross(points[(k + 1) % points.len()]);
            }
            let middle = points.iter().copied().sum::<Point3>() / points.len().max(1) as f64;
            Ring {
                area: normal.length() / 2.0,
                normal: normal.normalize_or_zero(),
                uses,
                points,
                middle,
            }
        })
        .collect();
    ensure!(
        all.iter().all(|r| r.area > near * near),
        "a curve closes around no area"
    );
    // Largest first: what each lies in is before it.
    all.sort_by(|a, b| b.area.total_cmp(&a.area));
    let flat = |of: &Ring, p: Point3| {
        let x = of.normal.any_orthonormal_vector();
        let y = of.normal.cross(x);
        let d = p - of.middle;
        workpiece_core::math::Point2::new(d.dot(x), d.dot(y))
    };
    let within = |of: &Ring, ring: &Ring| {
        let reach = of
            .points
            .iter()
            .map(|p| p.distance(of.middle))
            .fold(0.0_f64, f64::max);
        let polygon: Vec<_> = of.points.iter().map(|p| flat(of, *p)).collect();
        (ring.middle - of.middle).dot(of.normal).abs() <= reach
            && inside(&polygon, flat(of, ring.points[0]))
    };
    // Outer rings, each with the holes directly in it; a ring in a hole
    // is an outer ring again.
    let mut outer_of: Vec<Option<usize>> = Vec::new();
    let mut is_hole: Vec<bool> = Vec::new();
    for k in 0..all.len() {
        let container = (0..k).rev().find(|&j| within(&all[j], &all[k]));
        let hole = container.is_some_and(|j| !is_hole[j]);
        is_hole.push(hole);
        outer_of.push(container.filter(|_| hole));
    }
    let mut profiles = Vec::new();
    for k in (0..all.len()).filter(|&k| !is_hole[k]) {
        let ring = &all[k];
        let x = ring.normal.any_orthonormal_vector();
        let plane = workpiece_core::world::DatumPlane::new(ring.middle, ring.normal, x)?;
        let mut profile = Profile::new(plane);
        let holes: Vec<usize> = (0..all.len()).filter(|&j| outer_of[j] == Some(k)).collect();
        let walk = |ring: &Ring,
                    counter: bool,
                    profile: &mut Profile|
         -> anyhow::Result<Vec<(usize, bool)>> {
            let place = |p: Point3| flat_on(&plane, p);
            // Counter-clockwise in the plane for an outer ring,
            // clockwise for a hole.
            let polygon: Vec<_> = ring.points.iter().map(|p| place(*p)).collect();
            let turned = (signed_area(&polygon) > 0.0) != counter;
            let mut uses = ring.uses.clone();
            if turned {
                uses.reverse();
                for u in &mut uses {
                    u.1 = !u.1;
                }
            }
            let first_vertex = profile.vertices.len();
            let mut walk = Vec::new();
            for (n, &(k, reversed)) in uses.iter().enumerate() {
                let (c, t0, t1) = &pieces[k];
                let flat_curve = workpiece_core::world::topo::Curve2::new(
                    c.degree(),
                    c.knots().to_vec(),
                    c.control_points().iter().map(|p| place(*p)).collect(),
                    Some(c.weights().to_vec()),
                )?;
                let from = if reversed { *t1 } else { *t0 };
                profile.vertices.push(workpiece_core::world::ProfileVertex {
                    name: format!("p{}", first_vertex + n),
                    point: place(c.point_at(from)),
                    tolerance: near,
                });
                let (a, b) = (first_vertex + n, first_vertex + (n + 1) % uses.len());
                let (start, end) = if reversed { (b, a) } else { (a, b) };
                let e = profile.edges.len();
                profile.edges.push(workpiece_core::world::ProfileEdge {
                    name: format!("e{e}"),
                    entity: format!("e{e}"),
                    span: (0.0, 1.0),
                    curve: flat_curve,
                    t0: *t0,
                    t1: *t1,
                    start,
                    end,
                    tolerance: near,
                });
                walk.push((e, reversed));
            }
            Ok(walk)
        };
        let outer = walk(ring, true, &mut profile)?;
        let mut inner = Vec::new();
        for &j in &holes {
            inner.push(walk(&all[j], false, &mut profile)?);
        }
        profile.regions.push(workpiece_core::world::ProfileRegion {
            outer,
            holes: inner,
            area: ring.area,
        });
        profiles.push(profile);
    }
    Ok(profiles)
}

/// `p` in the coordinates of `plane` (flattened onto it).
fn flat_on(plane: &workpiece_core::world::DatumPlane, p: Point3) -> workpiece_core::math::Point2 {
    let d = p - plane.origin();
    workpiece_core::math::Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
}

/// The area `polygon` encloses, more than nothing counter-clockwise.
fn signed_area(polygon: &[workpiece_core::math::Point2]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|k| polygon[k].perp_dot(polygon[(k + 1) % n]))
        .sum::<f64>()
        / 2.0
}

/// Whether `p` is inside `polygon` (by the crossings of a ray from it).
fn inside(polygon: &[workpiece_core::math::Point2], p: workpiece_core::math::Point2) -> bool {
    let n = polygon.len();
    let mut odd = false;
    for k in 0..n {
        let (a, b) = (polygon[k], polygon[(k + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            odd = !odd;
        }
    }
    odd
}

impl Operation for Emboss {
    type Args = EmbossArgs;
    type EditState = ();

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &EmbossArgs {
        &self.args
    }

    fn args_mut(&mut self) -> &mut EmbossArgs {
        &mut self.args
    }

    fn make(&self, builder: &dyn Builder) -> anyhow::Result<workpiece_core::Made<'_>> {
        // (Nothing is changed here: the bodies it makes are written by
        // the builder.)
        let mut out = workpiece_shape::bodies::Out::default();
        let args = &self.args;
        let height = (*builder).measure(&args.height, "the height")?;
        ensure!(height > 0.0, "by some height");
        ensure!(!args.faces.is_empty(), "pick the faces to emboss");
        let world = builder.tracked();
        let topo = &world.topo;
        // What is raised: the areas of the sketch, or what the curves
        // bound (each outer ring with its holes, a profile of its own).
        let (sketch, regions, rings) = match args.outline {
            Outline::Sketch => {
                let sketch = args.sketch.as_ref().context("pick a sketch")?.name();
                let profile = world
                    .profiles
                    .get(sketch)
                    .with_context(|| format!("there is no sketch {sketch:?}"))?
                    .clone();
                let regions = sweep::regions(&profile, &args.areas)?;
                ensure!(
                    !regions.is_empty(),
                    "the sketch {sketch:?} has no closed area{}",
                    profile.open_note()
                );
                (Some(profile), regions, Vec::new())
            }
            Outline::Curves => {
                let names: Vec<&str> = args.curves.iter().map(|c| c.name()).collect();
                (None, Vec::new(), curve_profiles(topo, &names)?)
            }
        };
        let square = args.normal_to_face || args.outline == Outline::Curves;
        let picked: Vec<&str> = args.faces.iter().map(|r| r.name()).collect();
        let mut found = vec![false; picked.len()];
        for (body, name) in workpiece_shape::bodies::solids_with(topo, &picked) {
            let solid = read(topo, body)?;
            let faces: Vec<usize> = (0..solid.faces.len())
                .filter(|&f| {
                    let named = solid.faces[f].name.as_deref();
                    let also = solid.faces[f].also.iter().map(String::as_str);
                    let hit = named
                        .into_iter()
                        .chain(also)
                        .find_map(|n| picked.iter().position(|p| *p == n));
                    if let Some(k) = hit {
                        found[k] = true;
                    }
                    hit.is_some()
                })
                .collect();
            if faces.is_empty() {
                continue;
            }
            let areas = match &sketch {
                Some(profile) => sketch_areas(&self.id, &solid, &faces, profile, &regions),
                // (A ring's area is met where the ring is: around the
                // plane through its middle.)
                None => rings
                    .iter()
                    .enumerate()
                    .map(|(k, profile)| Area {
                        profile,
                        region: 0,
                        name: format!("{}/a{k}", self.id),
                        toward: Frame::of(profile).normal(),
                    })
                    .collect(),
            };
            let by = if args.engrave { -height } else { height };
            if out.made.annotations.is_empty() {
                out.made.annotations.push(workpiece_shape::on::held(
                    &self.id,
                    "height",
                    (&solid, faces[0]),
                    (by, None),
                    &args.height,
                    height,
                ));
            }
            let made = embossed(&solid, &faces, &areas, by, square)?;
            out.remove(topo, body);
            out.write(&made, &name);
        }
        if let Some(k) = found.iter().position(|f| !f) {
            anyhow::bail!("there is no face {:?} to emboss", picked[k]);
        }
        Ok(out.made)
    }

    fn summary(&self) -> String {
        let what = if self.args.engrave { "sunk" } else { "raised" };
        format!("{what} by {}", self.args.height)
    }

    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        (): &(),
    ) -> EditVisualization<Edit<EmbossArgs, ()>> {
        let args = &self.args;
        // The fields the arguments give; the handle is its own.
        let fields = workpiece_core::Arguments::fields(args, context);
        let mut handles = BTreeMap::new();
        if let Some(profile) =
            ui::profile(context.world, &args.sketch).filter(|_| args.outline == Outline::Sketch)
        {
            let picker = ui::area_picker(
                profile,
                &args.areas,
                ui::set(args, |a, areas| a.areas = areas),
            );
            handles.insert("areas".to_owned(), picker);
        }
        EditVisualization::new(VBox::new(fields), handles).map(Edit::args)
    }
}

/// The field that picks the sketch (what was picked of another sketch
/// goes with it), and what of it is taken.
fn sketch_fields(args: &EmbossArgs, _: &EditContext<'_>) -> Vec<Widget<EmbossArgs>> {
    let set = ui::set(args, |a, sketch| {
        a.sketch = sketch;
        a.areas.clear();
    });
    ui::sketch_fields(&args.sketch, args.areas.len(), set)
}

/// The box of the curves that bound what is raised: edges, or curves in
/// space (wire bodies).
fn curves_field(args: &EmbossArgs, _: &EditContext<'_>) -> Vec<Widget<EmbossArgs>> {
    let set = ui::set(args, |a, picked: Vec<workpiece_core::Source>| {
        a.curves = workpiece_core::Reference::each(picked)
    });
    vec![
        workpiece_core::widgets::ReferenceList::new(
            "Curves",
            vec![
                workpiece_core::ReferenceKind::Edge,
                workpiece_core::ReferenceKind::Body,
            ],
            workpiece_core::Reference::sources(&args.curves),
            set,
        )
        .required()
        .into(),
    ]
}
