//! What steps that work within outlines on faces share (Emboss, the
//! split of a surface): the outlines — the areas of a sketch thrown
//! onto the faces along its normal, or closed curves that lie on them
//! already — and where they meet the faces first.

use anyhow::{Context, ensure};
use workpiece_boolean::{Op, Solid, combine, read};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::Topo;
use workpiece_core::world::{AreaPick, Profile, World};
use workpiece_core::{Anything, Reference, Sketched};

use crate::sweep::{self, Frame, Make};

/// What the outlines are: the areas of a sketch thrown onto the faces,
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

/// One area: a region of a profile, thrown onto the faces along the
/// profile's normal; and where it is first met from.
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

/// The outlines a step was given, read: the sketch's profile and the
/// regions of it taken, or the profiles the closed curves bound (each
/// outer ring with its holes; the area is its only region).
pub struct Outlines {
    /// The sketch, for outlines of a sketch.
    pub sketch: Option<Profile>,
    /// Its regions taken.
    pub regions: Vec<usize>,
    /// The rings of curves, for outlines of curves.
    pub rings: Vec<Profile>,
}

impl Outlines {
    /// The outlines `outline` says, of `sketch` (its `areas`, none for
    /// all of it) or of `curves`, in `world`.
    pub fn read(
        world: &World,
        outline: Outline,
        sketch: &Option<Reference<Sketched>>,
        areas: &[AreaPick],
        curves: &[Reference<Anything>],
    ) -> anyhow::Result<Self> {
        Ok(match outline {
            Outline::Sketch => {
                let sketch = sketch.as_ref().context("pick a sketch")?.name();
                let profile = world
                    .profiles
                    .get(sketch)
                    .with_context(|| format!("there is no sketch {sketch:?}"))?
                    .clone();
                let regions = sweep::regions(&profile, areas)?;
                ensure!(
                    !regions.is_empty(),
                    "the sketch {sketch:?} has no closed area{}",
                    profile.open_note()
                );
                Self {
                    sketch: Some(profile),
                    regions,
                    rings: Vec::new(),
                }
            }
            Outline::Curves => {
                let names: Vec<&str> = curves.iter().map(|c| c.name()).collect();
                Self {
                    sketch: None,
                    regions: Vec::new(),
                    rings: curve_profiles(&world.topo, &names)?,
                }
            }
        })
    }

    /// The areas, to throw onto `solid`'s faces `faces`; named under
    /// `id`.
    pub fn areas(&self, id: &str, solid: &Solid, faces: &[usize]) -> Vec<Area<'_>> {
        match &self.sketch {
            Some(profile) => sketch_areas(id, solid, faces, profile, &self.regions),
            // (A ring's area is met where the ring is: around the plane
            // through its middle.)
            None => self
                .rings
                .iter()
                .enumerate()
                .map(|(k, profile)| Area {
                    profile,
                    region: 0,
                    name: format!("{id}/a{k}"),
                    toward: Frame::of(profile).normal(),
                })
                .collect(),
        }
    }
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

/// How far the faces of `solid` reach from `origin`, and more: a prism
/// that long each way goes through all of them.
pub fn reach(solid: &Solid, origin: Point3) -> f64 {
    let mut far = 0.0_f64;
    for face in &solid.faces {
        for p in face.surface.control_points() {
            far = far.max(p.distance(origin));
        }
    }
    far * 1.5 + 1.0
}

/// `area` swept along its profile's normal from `from` to `to` (along
/// the normal, from the profile's plane), as a solid named under the
/// area's name.
pub fn prism(area: &Area<'_>, (from, to): (f64, f64)) -> anyhow::Result<Solid> {
    let walks = sweep::outline(area.profile, &[area.region])?;
    let mut scratch = Topo::new();
    let through = sweep::extrude(
        &mut scratch,
        &area.name,
        area.profile,
        &walks,
        true,
        (from, to),
        Make::Solid,
    )?;
    Ok(read(&scratch, through)?)
}

/// Of `lumps`, those met first going from `origin` along `toward`: the
/// nearest one ahead (or around `origin`), and those as near as it
/// reaches — not those behind it, on the far side of the body.
pub fn first_met(lumps: Vec<Solid>, origin: Point3, toward: Point3) -> Vec<Solid> {
    let ahead: Vec<(Solid, (f64, f64))> = lumps
        .into_iter()
        .map(|l| {
            let s = span(&l, origin, toward);
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

/// How far along `toward` from `origin` the vertices of `lump` are: the
/// nearest and the farthest.
fn span(lump: &Solid, origin: Point3, toward: Point3) -> (f64, f64) {
    lump.vertices
        .iter()
        .map(|v| (v.point - origin).dot(toward))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| {
            (lo.min(d), hi.max(d))
        })
}

/// `solid` (a sheet or a solid) with its faces `faces` split along the
/// outlines `areas`: each area cuts what of the faces it meets first
/// into what is inside it — new faces named after the area
/// (`{name}`, `{name}/2`, …) — and what is outside, which keeps the
/// faces' names. Returns the body and whether it is closed (a solid).
pub fn split_faces(
    solid: &Solid,
    faces: &[usize],
    areas: &[Area<'_>],
) -> anyhow::Result<(Solid, bool)> {
    ensure!(!faces.is_empty(), "pick the faces to split");
    ensure!(!areas.is_empty(), "nothing to split them along");
    let picked = crate::surface::faces_of(solid, faces);
    let (lo, hi) = picked.extent();
    let size = (hi - lo).length();
    ensure!(size > 0.0, "the faces have some size");
    // The faces as a thin solid, cut as solids are; of what is left,
    // the faces in the faces' own surfaces.
    let thin_by = size * 1e-3;
    let all: Vec<usize> = (0..picked.faces.len()).collect();
    let thin = crate::thick::thicken(&picked, &all, thin_by, false, true)
        .context("following the faces")?;
    let mut inside: Vec<Solid> = Vec::new();
    let mut outside = thin.clone();
    for area in areas {
        let frame = Frame::of(area.profile);
        let (origin, normal) = (frame.origin, frame.normal());
        let far = reach(solid, origin) + size;
        // Where the area meets the faces first: the prism only as deep
        // as that, so it cuts nothing behind (the far side of a ring).
        let through = prism(area, (-far, far))?;
        let under = combine(Op::Intersect, &thin, &through)
            .context("throwing an area onto the faces")?
            .solid;
        let met = first_met(under.lumps()?, origin, area.toward);
        ensure!(!met.is_empty(), "an outline is not over the faces");
        let (near, deep) = met
            .iter()
            .map(|l| span(l, origin, area.toward))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (lo, hi)| {
                (a.min(lo), b.max(hi))
            });
        let pad = thin_by * 4.0 + (deep - near) * 0.01;
        let (near, deep) = (near - pad, deep + pad);
        // (Along the normal, as the prism is swept.)
        let along = if area.toward.dot(normal) < 0.0 {
            (-deep, -near)
        } else {
            (near, deep)
        };
        let cutter = prism(area, along)?;
        let piece = combine(Op::Intersect, &thin, &cutter)
            .context("cutting the faces along an outline")?
            .solid;
        let kept = tagged(&piece);
        let mut piece = crate::surface::faces_of(&piece, &kept);
        for (k, face) in piece.faces.iter_mut().enumerate() {
            let was = crate::surface::tag_of(face).map(|f| &picked.faces[f]);
            face.outward = was.is_none_or(|w| w.outward);
            face.name = Some(match k {
                0 => area.name.clone(),
                _ => format!("{}/{}", area.name, k + 1),
            });
            face.also = Vec::new();
        }
        inside.push(piece);
        outside = combine(Op::Cut, &outside, &cutter)
            .context("cutting the faces along an outline")?
            .solid;
    }
    // What is outside the outlines: the faces as they were called.
    let kept = tagged(&outside);
    let mut outside = crate::surface::faces_of(&outside, &kept);
    let mut seen = std::collections::BTreeMap::<usize, usize>::new();
    for face in &mut outside.faces {
        if let Some(f) = crate::surface::tag_of(face) {
            let was = &picked.faces[f];
            let n = seen.entry(f).or_default();
            *n += 1;
            face.outward = was.outward;
            face.name = match (*n, &was.name) {
                (1, name) => name.clone(),
                (n, Some(name)) => Some(format!("{name}/{n}")),
                (_, None) => None,
            };
            face.also = if *n == 1 {
                was.also.clone()
            } else {
                Vec::new()
            };
        }
    }
    let rest: Vec<usize> = (0..solid.faces.len())
        .filter(|f| !faces.contains(f))
        .collect();
    let mut parts = vec![outside];
    parts.extend(inside);
    if !rest.is_empty() {
        parts.push(crate::surface::faces_of(solid, &rest));
    }
    let (sewn, closed) =
        workpiece_boolean::stitched(&parts, size * 1e-6).context("sewing the split faces")?;
    Ok((sewn, closed))
}

/// The faces of `cut` (a thin solid cut) that are pieces of the faces it
/// was thickened from.
fn tagged(cut: &Solid) -> Vec<usize> {
    (0..cut.faces.len())
        .filter(|&f| crate::surface::tag_of(&cut.faces[f]).is_some())
        .collect()
}

/// The pieces of curve `curves` (edges of any body, curve bodies — all
/// of their edges) of `topo` are: each its curve and the parameters it
/// is between.
pub fn curve_pieces(
    topo: &Topo,
    curves: &[&str],
) -> anyhow::Result<Vec<(workpiece_core::world::topo::Curve3, f64, f64)>> {
    use workpiece_core::world::topo::{BodyId, BodyKind, EdgeId};
    let mut pieces = Vec::new();
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
    Ok(pieces)
}

/// The normal of the plane all of `pieces` lie in, if they lie in one
/// (within a millionth of their size), by the way they turn around it.
pub fn plane_of(
    pieces: &[(workpiece_core::world::topo::Curve3, f64, f64)],
) -> Option<(Point3, Point3)> {
    let points: Vec<Point3> = pieces
        .iter()
        .flat_map(|(c, t0, t1)| c.polyline(*t0, *t1))
        .collect();
    let first = *points.first()?;
    let middle = points.iter().copied().sum::<Point3>() / points.len() as f64;
    let size = points
        .iter()
        .map(|p| p.distance(first))
        .fold(0.0_f64, f64::max);
    let mut normal = Point3::ZERO;
    for (k, p) in points.iter().enumerate() {
        normal += (*p - middle).cross(points[(k + 1) % points.len()] - middle);
    }
    let normal = normal.try_normalize()?;
    points
        .iter()
        .all(|p| (*p - middle).dot(normal).abs() <= size * 1e-6)
        .then_some((middle, normal))
}

/// The closed curves `curves` (edges, wire bodies) of `topo`, as areas:
/// each outer ring of them with the rings inside it, flattened along its
/// own normal onto the plane through its middle. Returns the profiles;
/// the areas are their only regions.
pub fn curve_profiles(topo: &Topo, curves: &[&str]) -> anyhow::Result<Vec<Profile>> {
    let pieces = curve_pieces(topo, curves)?;
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
        Point2::new(d.dot(x), d.dot(y))
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
fn flat_on(plane: &workpiece_core::world::DatumPlane, p: Point3) -> Point2 {
    let d = p - plane.origin();
    Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
}

/// The area `polygon` encloses, more than nothing counter-clockwise.
fn signed_area(polygon: &[Point2]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|k| polygon[k].perp_dot(polygon[(k + 1) % n]))
        .sum::<f64>()
        / 2.0
}

/// Whether `p` is inside `polygon` (by the crossings of a ray from it).
fn inside(polygon: &[Point2], p: Point2) -> bool {
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

/// The direction `reference` gives, in `world`: an axis's (a datum axis,
/// a frame's axis, a straight edge, a round face's axis), or a plane's
/// normal (a datum plane, a flat face).
pub fn direction_of(world: &World, reference: &Reference<Anything>) -> anyhow::Result<Point3> {
    use workpiece_core::world::{DatumAxis, DatumPlane};
    let source = reference.source.clone();
    if let Ok(axis) = world.resolve(&Reference::<DatumAxis>::new(source.clone())) {
        return Ok(axis.direction());
    }
    let plane = world
        .resolve(&Reference::<DatumPlane>::new(source))
        .map_err(|_| {
            anyhow::anyhow!(
                "{} gives no direction: pick an axis, a straight edge, a plane or a flat face",
                reference.name()
            )
        })?;
    Ok(plane.normal())
}

/// A sheet of the curves `pieces`, each swept by `sweep` (given the
/// curve, its range, and a name for its face under `id`), sewn where
/// they meet: connected curves give connected faces.
pub fn swept_curves(
    pieces: &[(workpiece_core::world::topo::Curve3, f64, f64)],
    id: &str,
    sweep: impl Fn(&workpiece_core::world::topo::Curve3, (f64, f64), &str) -> anyhow::Result<Solid>,
) -> anyhow::Result<Solid> {
    ensure!(!pieces.is_empty(), "pick the curves to sweep");
    let mut sheets = Vec::new();
    for (k, (curve, t0, t1)) in pieces.iter().enumerate() {
        sheets.push(sweep(curve, (*t0, *t1), &format!("{id}/side/c{k}"))?);
    }
    let size = sheets
        .iter()
        .map(|s| {
            let (lo, hi) = s.extent();
            (hi - lo).length()
        })
        .fold(0.0, f64::max);
    let (sewn, _) = workpiece_boolean::stitched(&sheets, size * 1e-7)?;
    Ok(sewn)
}

/// The closed curves `curves` as the one profile a solid is swept from:
/// they have to close around an area (with holes in it) and lie in one
/// plane.
pub fn solid_profile(topo: &Topo, curves: &[&str]) -> anyhow::Result<Profile> {
    let refused = "curves make a solid only where they close around an area and lie in one plane: make a surface of them";
    let pieces = curve_pieces(topo, curves)?;
    ensure!(plane_of(&pieces).is_some(), "{refused}");
    let mut profiles = curve_profiles(topo, curves).context(refused)?;
    ensure!(
        profiles.len() == 1,
        "the curves close around {} areas apart: pick those of one (and of the holes in it)",
        profiles.len()
    );
    Ok(profiles.remove(0))
}
