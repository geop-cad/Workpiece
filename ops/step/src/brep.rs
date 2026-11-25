//! The solids of a STEP file as B-reps of this kernel.
//!
//! A STEP face is a surface and loops of edges in space. Here a face is
//! also a region of its surface's (u, v), bounded by the loops' curves
//! there. So for every face its surface is built as a patch that covers
//! it, and every edge on it gets its curve in (u, v), its points' feet on
//! the patch. A face that goes all the way around an axis has no single
//! region there; it is cut open along a seam, a new edge where the patch
//! begins and ends.
//!
//! What cannot be read is refused, with the reason: a face around a
//! pole, a surface kind not known, an edge that is not on its face.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use anyhow::{Context, bail, ensure};
use workpiece_boolean::geometry::{foot, pcurve};
use workpiece_boolean::model::{Edge, Face, Solid, Use, Vertex, Walk};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::topo::{Curve2, Curve3, NurbsSurface};

use crate::assembly::Assembly;
use crate::geometry::{
    CurveKind, Patch, Reader, SurfaceKind, closes, closes_across, collapses, edge_curve, locate,
    near, swapped,
};
use crate::part21::{File, Value};

/// A solid of a file.
pub struct Body {
    /// What the file calls it, if anything.
    pub name: String,
    /// The solid.
    pub solid: Solid,
}

/// What a file holds.
pub struct Imported {
    /// The solids that could be read.
    pub bodies: Vec<Body>,
    /// Those that could not, each with why.
    pub refused: Vec<String>,
}

/// How many points of an edge are looked at to see where it runs.
const SAMPLES: usize = 16;

/// An edge of the file: its curve from its first vertex to its second,
/// and where it has to be cut.
struct Rim {
    curve: Arc<Curve3>,
    range: (f64, f64),
    ends: [usize; 2],
    cuts: Vec<(f64, usize)>,
    /// How far along it (0 at its start, 1 at its end) it goes through
    /// a point some surface collapses to: where it is looked at exactly,
    /// because the angle around that surface's axis jumps there.
    poles: Vec<f64>,
    /// The edges of the solid it becomes, in its direction.
    pieces: Vec<usize>,
}

impl Rim {
    /// The point `f` of the way along it, walked backwards if
    /// `reversed`.
    fn at(&self, reversed: bool, f: f64) -> Point3 {
        let f = if reversed { 1.0 - f } else { f };
        self.curve
            .point_at(self.range.0 + (self.range.1 - self.range.0) * f)
    }
}

type Loop = Vec<(usize, bool)>;

/// Where a face that goes all around its axis is cut open.
enum Seam {
    /// Between two loops: the one that runs the way the angle rises,
    /// the one that runs against it, and the vertex of that one on the
    /// seam.
    Band {
        plus: usize,
        minus: usize,
        vertex: usize,
        /// The vertex the first of the loops starts at, on the seam,
        /// if that is not where it starts in the file.
        first: Option<usize>,
    },
    /// From its one loop to the point its surface collapses to (a
    /// cone's tip, a sphere's pole): at the end of the profile if
    /// `above`, else at its start.
    Cap { above: bool },
    /// From pole to pole: a whole sphere.
    Globe,
}

enum Shape {
    Flat([f64; 4]),
    Whole,
    Turned {
        from: f64,
        over: f64,
        part: (f64, f64),
        /// The angle each loop starts at.
        starts: Vec<f64>,
        seam: Option<Seam>,
        /// On a surface closed both ways: where the face lies around
        /// its second parameter.
        across: Option<Across>,
    },
}

/// Where a face on a surface closed both ways lies around the second
/// parameter, as angles (a whole turn from side to side).
struct Across {
    /// The angle of the surface's seam the face's loops lie above,
    /// within a turn.
    base: f64,
    /// Of each loop: the angle it starts at, and the angles it lies
    /// between.
    loops: Vec<(f64, (f64, f64))>,
}

/// The angles around the second parameter of a surface closed both ways
/// that the loops through `points` run at, each followed without jumps.
/// No loop may go around that way (the face is then cut open nowhere),
/// and all lie within one turn from the surface's seam.
fn across(kind: &SurfaceKind, points: &[Vec<Vec<Point3>>]) -> anyhow::Result<Across> {
    let mut walks: Vec<(f64, (f64, f64))> = Vec::new();
    for lp in points {
        let mut seen: Option<(f64, f64)> = None;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in lp.iter().flatten() {
            let raw = kind
                .around_both(*p)
                .context("an edge is not on its surface")?
                .1;
            let (first, last) = seen.unwrap_or((raw, raw));
            let at = unwrap(raw, last);
            seen = Some((first, at));
            (lo, hi) = (lo.min(at), hi.max(at));
        }
        let (first, last) = seen.context("an empty loop")?;
        ensure!(
            (last - first).abs() < 1.0,
            "a face around a surface closed both ways, with no edge along its seam"
        );
        walks.push((first, (lo, hi)));
    }
    // Side by side around the first, as the loops are around the axis.
    let (mut lo, mut hi) = walks.first().context("a closed face without edges")?.1;
    let centre = (lo + hi) / 2.0;
    for (start, (a, b)) in &mut walks[1..] {
        let shift = unwrap((*a + *b) / 2.0, centre) - (*a + *b) / 2.0;
        (*start, *a, *b) = (*start + shift, *a + shift, *b + shift);
        (lo, hi) = (lo.min(*a), hi.max(*b));
    }
    let base = TAU * ((lo + 1e-6) / TAU).floor();
    ensure!(
        hi <= base + TAU + 1e-6,
        "a face across the seam of its surface"
    );
    for (start, (a, b)) in &mut walks {
        (*a, *b) = (a.max(base), b.min(base + TAU));
        *start = start.clamp(*a, *b);
    }
    Ok(Across { base, loops: walks })
}

struct Plan {
    id: u32,
    kind: SurfaceKind,
    outward: bool,
    loops: Vec<Loop>,
    shape: Shape,
}

struct Builder<'r, 'a> {
    r: &'r Reader<'a>,
    /// The corners of the box around the body.
    corners: Vec<Point3>,
    size: f64,
    tol: f64,
    vertices: Vec<Vertex>,
    vmap: HashMap<u32, usize>,
    rims: Vec<Rim>,
    rmap: HashMap<u32, usize>,
    /// The vertex at each place one has been read at.
    places: HashMap<[u64; 3], usize>,
    edges: Vec<Edge>,
}

/// `raw`, moved by whole turns to be nearest to `near`.
fn unwrap(raw: f64, near: f64) -> f64 {
    raw + TAU * ((near - raw) / TAU).round()
}

/// The angle a loop comes to at `raw` after `last`. Usually the nearest;
/// but through a point on the axis (`pole`: whether that is where the
/// profile starts) the angle jumps, and the loop, which has its face on
/// the left, goes on at the next angle above (around the profile's
/// start) or below (around its end) — a whole turn on if it is the same.
fn onward(last: f64, raw: f64, pole: Option<bool>) -> f64 {
    match pole {
        None => unwrap(raw, last),
        Some(true) => {
            let at = raw + TAU * ((last - raw) / TAU).ceil();
            if at - last < 1e-6 { at + TAU } else { at }
        }
        Some(false) => {
            let at = raw + TAU * ((last - raw) / TAU).floor();
            if last - at < 1e-6 { at - TAU } else { at }
        }
    }
}

impl<'r, 'a> Builder<'r, 'a> {
    fn vertex(&mut self, id: u32) -> anyhow::Result<usize> {
        if let Some(&v) = self.vmap.get(&id) {
            return Ok(v);
        }
        let args = self.r.get(id)?.own();
        let point = self
            .r
            .point(args.reference(1).context("a vertex has no point")?)?;
        // A second vertex at the very place of another is that one.
        let place = point.to_array().map(|x| (x + 0.0).to_bits());
        if let Some(&v) = self.places.get(&place) {
            self.vmap.insert(id, v);
            return Ok(v);
        }
        self.places.insert(place, self.vertices.len());
        self.vmap.insert(id, self.vertices.len());
        self.vertices.push(Vertex {
            point,
            tolerance: self.tol,
            name: None,
        });
        Ok(self.vertices.len() - 1)
    }

    /// Makes the vertices `a` and `b`, the ends of an edge with no
    /// length, one: `b` is `a` from now on.
    fn merge(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        let apart = self.vertices[a].point.distance(self.vertices[b].point);
        self.vertices[a].tolerance = self.vertices[a].tolerance.max(self.tol + apart);
        let to = |v: &mut usize| {
            if *v == b {
                *v = a;
            }
        };
        self.vmap.values_mut().for_each(to);
        for rim in &mut self.rims {
            rim.ends.iter_mut().for_each(to);
            rim.cuts.iter_mut().for_each(|c| to(&mut c.1));
        }
        for edge in &mut self.edges {
            edge.ends.iter_mut().for_each(to);
        }
    }

    fn rim(&mut self, id: u32) -> anyhow::Result<usize> {
        if let Some(&e) = self.rmap.get(&id) {
            return Ok(e);
        }
        let args = self.r.get(id)?.own();
        let ends = [
            self.vertex(args.reference(1).context("an edge has no start")?)?,
            self.vertex(args.reference(2).context("an edge has no end")?)?,
        ];
        // Two vertices at one place are one: the edge closes on itself.
        let ends = if ends[0] != ends[1]
            && self.vertices[ends[0]]
                .point
                .distance(self.vertices[ends[1]].point)
                <= self.tol
        {
            self.merge(ends[0], ends[1]);
            [ends[0]; 2]
        } else {
            ends
        };
        let kind = self
            .r
            .curve(args.reference(3).context("an edge has no curve")?)?;
        let same = args.flag(4).unwrap_or(true);
        // An end is on its curve within what the file says is near —
        // of a body's size, no more than is near here for a face's
        // edge — or within what its vertex is wide (one that edges of
        // no length were drawn together into is as far from where the
        // file had it).
        let within = ends
            .iter()
            .map(|v| self.vertices[*v].tolerance)
            .fold(self.tol, f64::max)
            .max(self.r.closeness.min(self.size * 1e-3));
        let (curve, range) = edge_curve(
            &kind,
            self.vertices[ends[0]].point,
            self.vertices[ends[1]].point,
            ends[0] == ends[1],
            same,
            within,
        )
        .with_context(|| format!("edge #{id}"))?;
        self.rmap.insert(id, self.rims.len());
        self.rims.push(Rim {
            curve: Arc::new(curve),
            range,
            ends,
            cuts: Vec::new(),
            poles: Vec::new(),
            pieces: Vec::new(),
        });
        Ok(self.rims.len() - 1)
    }

    /// The loops of face `id`, each walked with the face on its left
    /// seen from where the surface's normal points.
    fn loops(
        &mut self,
        bounds: crate::part21::Arguments<'a>,
        same: bool,
    ) -> anyhow::Result<Vec<Loop>> {
        let mut loops = Vec::new();
        for bound in bounds.values().filter_map(|v| v.reference()) {
            let bound = self.r.get(bound)?.own();
            let walk = self
                .r
                .get(bound.reference(1).context("a bound has no loop")?)?;
            if walk.name() != "EDGE_LOOP" {
                // A loop that is one vertex bounds nothing.
                continue;
            }
            let mut steps = Vec::new();
            let list = walk.own().list(1).context("a loop has no edges")?;
            for oriented in list.values().filter_map(|v| v.reference()) {
                let oriented = self.r.get(oriented)?.own();
                let rim = self.rim(oriented.reference(3).context("no edge")?)?;
                // An edge with no length (a circle of no radius at a
                // cone's tip) is a point the loop passes through.
                let r = &self.rims[rim];
                let from = r.at(false, 0.0);
                // (So is a closed edge a millionth of the body across:
                // a pinhole where a surface comes to its axis.)
                // (An open one is of no length within what is near
                // here many times over — but no farther than what the
                // file itself says is near: a file written
                // as closely as this reads has edges that short, the
                // sides of slivers, and means them.)
                let near = self.tol * 16.0;
                let short = if r.ends[0] == r.ends[1] {
                    (self.size * 1e-6).max(near)
                } else if self.r.closeness > 0.0 {
                    self.r.closeness.clamp(self.tol * 2.0, near)
                } else {
                    near
                };
                let long = (1..=8).any(|k| r.at(false, f64::from(k) / 8.0).distance(from) > short);
                if long {
                    steps.push((rim, !oriented.flag(4).unwrap_or(true)));
                } else {
                    // Its two ends are one place.
                    let [a, b] = r.ends;
                    self.merge(a, b);
                }
            }
            if steps.is_empty() {
                continue;
            }
            let forward = bound.flag(2).unwrap_or(true) == same;
            if !forward {
                steps.reverse();
                for step in &mut steps {
                    step.1 = !step.1;
                }
            }
            loops.push(steps);
        }
        Ok(loops)
    }

    fn plan(&mut self, id: u32, flipped: bool) -> anyhow::Result<Plan> {
        let args = self.r.get(id)?.own();
        let surface = args.reference(2).context("a face has no surface")?;
        let mut kind = self.r.surface(surface, &self.corners)?;
        // A ring without a hole has an outer and an inner part: the
        // face is on the one its edges are on.
        if let Some(spindle) = self.r.spindle(surface)? {
            let rims = self.loops(args.list(1).context("a face has no bounds")?, true)?;
            let (mut outer, mut inner) = (0, 0);
            for &(rim, _) in rims.iter().flatten() {
                for f in [0.25, 0.5, 0.75] {
                    let p = self.rims[rim].at(false, f);
                    let (out, within) = (spindle.off(p, false), spindle.off(p, true));
                    if (out - within).abs() > self.tol * 16.0 {
                        *(if out < within { &mut outer } else { &mut inner }) += 1;
                    }
                }
            }
            if inner > outer {
                kind = spindle.part(true)?;
            }
        }
        // A NURBS surface that closes around is walked around like a
        // surface about an axis, its first parameter the angle.
        let kind = match kind {
            SurfaceKind::Spline(surface) => {
                let reach = self.size * 1e-3;
                match closes(&surface, self.tol) {
                    Some(false) => SurfaceKind::Closed {
                        both: closes_across(&surface, self.tol),
                        poles: collapses(&surface, self.size * 1e-6),
                        surface,
                        reach,
                        inward: false,
                    },
                    Some(true) => {
                        let surface = swapped(&surface)?;
                        SurfaceKind::Closed {
                            poles: collapses(&surface, self.size * 1e-6),
                            surface,
                            reach,
                            inward: true,
                            both: false,
                        }
                    }
                    None => SurfaceKind::Spline(surface),
                }
            }
            kind => kind,
        };
        // On a patch that faces the other way than the file's surface,
        // the face is the other way around too.
        let same = args.flag(3).unwrap_or(true) != kind.inward();
        let mut loops = self.loops(args.list(1).context("a face has no bounds")?, same)?;
        if let SurfaceKind::Turned { frame, .. } = &kind {
            // An edge through a point the surface collapses to (a
            // meridian of a sphere, through its pole) is cut there: on
            // the surface it comes to the pole at one angle and leaves
            // at another.
            let near = (self.size * 1e-6).max(self.tol * 4.0);
            for &(rim, _) in loops.iter().flatten() {
                let r = &self.rims[rim];
                let rho = |f: f64| frame.around(r.at(false, f)).1;
                const N: usize = 64;
                let mut found = Vec::new();
                let at = |i: usize| i as f64 / N as f64;
                let seen: Vec<(Point3, f64)> = (0..=N)
                    .map(|i| {
                        let p = r.at(false, at(i));
                        (p, frame.around(p).1)
                    })
                    .collect();
                for k in 1..N {
                    if seen[k].1 > seen[k - 1].1 || seen[k].1 > seen[k + 1].1 {
                        continue;
                    }
                    // (It comes no nearer the axis between two of these
                    // points than it runs between them.)
                    let run = seen[k - 1].0.distance(seen[k].0) + seen[k].0.distance(seen[k + 1].0);
                    if seen[k].1 > near + run {
                        continue;
                    }
                    let (mut lo, mut hi) = (at(k - 1), at(k + 1));
                    for _ in 0..80 {
                        let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                        if rho(a) < rho(b) {
                            hi = b;
                        } else {
                            lo = a;
                        }
                    }
                    let f = (lo + hi) / 2.0;
                    let p = r.at(false, f);
                    let ends = r.ends.map(|v| self.vertices[v].point);
                    if rho(f) <= near && ends.iter().all(|e| e.distance(p) > near * 4.0) {
                        found.push((f, p));
                    }
                }
                for (f, p) in found {
                    let t = {
                        let r = &mut self.rims[rim];
                        if !r.poles.iter().any(|g| (g - f).abs() <= 1e-9) {
                            r.poles.push(f);
                        }
                        r.range.0 + (r.range.1 - r.range.0) * f
                    };
                    let cut = self.rims[rim]
                        .cuts
                        .iter()
                        .any(|c| self.vertices[c.1].point.distance(p) <= near * 4.0);
                    if !cut {
                        self.vertices.push(Vertex {
                            point: p,
                            tolerance: self.tol,
                            name: None,
                        });
                        let v = self.vertices.len() - 1;
                        self.rims[rim].cuts.push((t, v));
                    }
                }
            }
        }
        if let Some(frame) = kind.axis() {
            // Not starting on the axis, where the angle is no angle.
            for lp in &mut loops {
                let off = lp.iter().position(|&(rim, reversed)| {
                    frame.around(self.rims[rim].at(reversed, 0.0)).1 > self.size * 1e-6
                });
                lp.rotate_left(off.unwrap_or(0));
            }
        }
        let samples = |b: &Self, lp: &Loop| -> Vec<Vec<Point3>> {
            lp.iter()
                .map(|&(rim, reversed)| {
                    b.fractions(rim, reversed)
                        .into_iter()
                        .map(|f| b.rims[rim].at(reversed, f))
                        .collect()
                })
                .collect()
        };
        let points: Vec<Vec<Vec<Point3>>> = loops.iter().map(|l| samples(self, l)).collect();
        let pad = |lo: f64, hi: f64| {
            let more = (hi - lo) * 0.15 + self.size * 1e-3;
            (lo - more, hi + more)
        };
        // A ring's face with a loop that goes all around its tube and
        // all around its axis and comes back (along its two seams, each
        // there and back) is closed both ways: it lies on the whole
        // ring, whose seams are put where that loop has them.
        let kind = match kind {
            SurfaceKind::Ring {
                frame,
                major,
                minor,
                inward,
            } => {
                let ring = SurfaceKind::Ring {
                    frame,
                    major,
                    minor,
                    inward,
                };
                let seams = points.iter().find_map(|lp| {
                    let mut seen: Option<[f64; 2]> = None;
                    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
                    for p in lp.iter().flatten() {
                        let (around, rho, h) = frame.around(*p);
                        let raw = [around, h.atan2(rho - major)];
                        let last = seen.unwrap_or(raw);
                        let at = [unwrap(raw[0], last[0]), unwrap(raw[1], last[1])];
                        seen = Some(at);
                        for k in 0..2 {
                            (lo[k], hi[k]) = (lo[k].min(at[k]), hi[k].max(at[k]));
                        }
                    }
                    (0..2).all(|k| hi[k] - lo[k] >= TAU - 1e-6).then_some(lo)
                });
                match seams {
                    Some([around, tube]) => SurfaceKind::Closed {
                        surface: ring.turned(around, TAU, (tube, tube + TAU))?.surface,
                        reach: self.size * 1e-3,
                        inward,
                        both: true,
                        poles: [None; 2],
                    },
                    None => ring,
                }
            }
            kind => kind,
        };
        // A ring's face that goes all around its tube is cut open along
        // the tube, not around the axis.
        let kind = match kind {
            SurfaceKind::Ring {
                frame,
                major,
                minor,
                inward,
            } if {
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                let mut last: Option<f64> = None;
                for p in points.iter().flatten().flatten() {
                    let raw = (frame.around(*p).2).atan2(frame.around(*p).1 - major);
                    let at = unwrap(raw, last.unwrap_or(raw));
                    last = Some(at);
                    (lo, hi) = (lo.min(at), hi.max(at));
                }
                hi - lo >= TAU - 1e-6
            } =>
            {
                SurfaceKind::Tube {
                    frame,
                    major,
                    minor,
                    inward,
                }
            }
            kind => kind,
        };
        let mut shape = match &kind {
            SurfaceKind::Spline(_) => Shape::Whole,
            SurfaceKind::Plane(frame) => {
                ensure!(!loops.is_empty(), "a plane without bounds");
                let (mut x, mut y) = (
                    (f64::INFINITY, f64::NEG_INFINITY),
                    (f64::INFINITY, f64::NEG_INFINITY),
                );
                for p in points.iter().flatten().flatten() {
                    let d = *p - frame.o;
                    x = (x.0.min(d.dot(frame.x)), x.1.max(d.dot(frame.x)));
                    y = (y.0.min(d.dot(frame.y)), y.1.max(d.dot(frame.y)));
                }
                let (x, y) = (pad(x.0, x.1), pad(y.0, y.1));
                Shape::Flat([x.0, x.1, y.0, y.1])
            }
            SurfaceKind::Turned { frame, profile } if loops.is_empty() => {
                // A whole sphere: its profile runs from the axis to the
                // axis.
                let (a, b) = profile.domain();
                let on_axis = |t: f64| frame.around(profile.point_at(t)).1 <= self.size * 1e-9;
                ensure!(on_axis(a) && on_axis(b), "a closed face without edges");
                Shape::Turned {
                    from: 0.0,
                    over: TAU,
                    part: (a, b),
                    starts: Vec::new(),
                    seam: Some(Seam::Globe),
                    across: None,
                }
            }
            SurfaceKind::Turned { .. }
            | SurfaceKind::Ring { .. }
            | SurfaceKind::Tube { .. }
            | SurfaceKind::Closed { .. } => {
                ensure!(!loops.is_empty(), "a closed face without edges");
                self.turned(&kind, &loops, &points)?
            }
        };
        if let Shape::Turned { across: at, .. } = &mut shape
            && kind.both()
        {
            *at = Some(across(&kind, &points)?);
        }
        Ok(Plan {
            id,
            outward: same != flipped,
            kind,
            loops,
            shape,
        })
    }

    /// How far along the rim `rim` (walked backwards if `reversed`) its
    /// points are looked at: evenly, but exactly where it goes through a
    /// pole.
    fn fractions(&self, rim: usize, reversed: bool) -> Vec<f64> {
        let mut at: Vec<f64> = (0..=SAMPLES).map(|k| k as f64 / SAMPLES as f64).collect();
        for &pole in &self.rims[rim].poles {
            let pole = if reversed { 1.0 - pole } else { pole };
            let nearest =
                (1..SAMPLES).min_by(|a, b| (at[*a] - pole).abs().total_cmp(&(at[*b] - pole).abs()));
            if let Some(k) = nearest
                && pole > at[k - 1]
                && pole < at[k + 1]
            {
                at[k] = pole;
            }
        }
        at
    }

    /// How a face on a surface around an axis lies on it: the angles it
    /// covers, and the seam it is cut open along if it goes all around.
    fn turned(
        &mut self,
        kind: &SurfaceKind,
        loops: &[Loop],
        points: &[Vec<Vec<Point3>>],
    ) -> anyhow::Result<Shape> {
        let frame = kind.axis().copied();
        let near_axis = self.size * 1e-6;
        // The angle along each loop, without jumps: of each walk, its
        // points' angles.
        let mut angles: Vec<Vec<Vec<f64>>> = Vec::new();
        let mut turns: Vec<i64> = Vec::new();
        for lp in points {
            let first = lp
                .iter()
                .flatten()
                .filter_map(|p| kind.turn(*p))
                .find(|a| a.1 > near_axis)
                .map_or(0.0, |a| a.0);
            let mut last = first;
            let mut pole = None;
            let mut seen = false;
            let mut walks = Vec::new();
            for walk in lp {
                let mut out = Vec::with_capacity(walk.len());
                for p in walk {
                    let (raw, rho) = kind.turn(*p).context("no axis")?;
                    if rho > near_axis {
                        // A loop that starts on the axis starts at its
                        // first angle, not a turn on from it.
                        if !std::mem::replace(&mut seen, true) {
                            pole = None;
                        }
                        last = onward(last, raw, pole.take());
                    } else {
                        pole = Some(self.at_start(kind, *p));
                    }
                    out.push(last);
                }
                walks.push(out);
            }
            // Back at the start, through the axis if the loop ends
            // there.
            let back = onward(last, first, pole);
            turns.push(((back - first) / TAU).round() as i64);
            angles.push(walks);
        }
        // The part of the profile (or of a ring's tube) the face needs.
        let part = match kind {
            SurfaceKind::Turned { profile, .. } => {
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                for p in points.iter().flatten().flatten() {
                    let t = kind.along_profile(*p).context("no profile")?;
                    (lo, hi) = (lo.min(t), hi.max(t));
                }
                let (a, b) = profile.domain();
                let more = (hi - lo) * 0.15 + (b - a) * 1e-3;
                let part = ((lo - more).max(a), (hi + more).min(b));
                ensure!(part.1 > part.0, "a face with no height");
                part
            }
            SurfaceKind::Closed { surface, .. } => surface.domain().1,
            SurfaceKind::Tube { frame, .. } => {
                // Around the axis, without jumps, loop after loop.
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                let mut last: Option<f64> = None;
                for p in points.iter().flatten().flatten() {
                    let raw = frame.around(*p).0;
                    let at = unwrap(raw, last.unwrap_or(raw));
                    last = Some(at);
                    (lo, hi) = (lo.min(at), hi.max(at));
                }
                ensure!(hi - lo < TAU - 1e-6, "a face all around a ring both ways");
                let more = ((hi - lo) * 0.15 + 1e-3)
                    .min((TAU - (hi - lo)) / 2.0)
                    .max(0.0);
                (lo - more, hi + more)
            }
            _ => {
                // Around the tube, without jumps, loop after loop.
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                let mut last: Option<f64> = None;
                for (k, lp) in points.iter().enumerate() {
                    let mut start = None;
                    for p in lp.iter().flatten() {
                        let raw = kind.around_tube(*p).context("no tube")?;
                        let mut at = unwrap(raw, last.unwrap_or(raw));
                        // Half a turn on from the loop before, either
                        // way around the tube: the way that has the
                        // loop that runs with the angle below the one
                        // that runs against it.
                        if let Some(before) = last
                            && start.is_none()
                            && ((at - before).abs() - PI).abs() < 1e-6
                            && turns[k] != 0
                        {
                            at = before + if turns[k] < 0 { PI } else { -PI };
                        }
                        last = Some(at);
                        start.get_or_insert(at);
                        (lo, hi) = (lo.min(at), hi.max(at));
                    }
                    let around = (last.unwrap_or(0.0) - start.unwrap_or(0.0)).abs();
                    ensure!(around < 1.0, "a face all around a ring's tube");
                }
                let more = ((hi - lo) * 0.15 + 1e-3)
                    .min((TAU - (hi - lo)) / 2.0)
                    .max(0.0);
                ensure!(hi - lo < TAU - 1e-6, "a face all around a ring's tube");
                (lo - more, hi + more)
            }
        };
        let around: Vec<usize> = (0..loops.len()).filter(|&k| turns[k] != 0).collect();
        if around.is_empty() {
            // Every loop closes without going around: side by side
            // around the first.
            let middle = |walks: &Vec<Vec<f64>>| {
                let all: Vec<f64> = walks.iter().flatten().copied().collect();
                let (lo, hi) = all.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |m, a| {
                    (m.0.min(*a), m.1.max(*a))
                });
                (lo, hi)
            };
            let (mut lo, mut hi) = middle(&angles[0]);
            let centre = (lo + hi) / 2.0;
            let mut starts = vec![angles[0][0][0]];
            for walks in &angles[1..] {
                let (a, b) = middle(walks);
                let shift = unwrap((a + b) / 2.0, centre) - (a + b) / 2.0;
                (lo, hi) = (lo.min(a + shift), hi.max(b + shift));
                starts.push(walks[0][0] + shift);
            }
            // (Edges that meet a hair past each other, as files have
            // them, make a whole turn a hair more.)
            ensure!(
                hi - lo <= TAU + 1e-4,
                "a face that goes {:.2} times around its axis: more than once",
                (hi - lo) / TAU
            );
            let hi = hi.min(lo + TAU);
            let (from, over) = if matches!(kind, SurfaceKind::Closed { .. }) {
                // The patch is the surface as it is, its seam where it
                // is: the face has to lie within one turn from there.
                // (To within what is near in the file, as an angle
                // about a body its size: edges that near the seam are
                // on it, and are found a hair to either side.)
                let slack =
                    (1e-6 + 16.0 * self.r.closeness / self.size.max(f64::MIN_POSITIVE)).min(1e-4);
                let base = TAU * ((lo + slack) / TAU).floor();
                ensure!(
                    hi <= base + TAU + slack,
                    "a face across the seam of its surface"
                );
                for start in &mut starts {
                    *start = start.clamp(base, base + TAU);
                }
                (base, TAU)
            } else if hi - lo > TAU - 1e-6 {
                (lo, TAU)
            } else {
                let more = ((hi - lo) * 0.1 + 1e-3).min((TAU - (hi - lo)) / 2.0);
                (lo - more, hi - lo + 2.0 * more)
            };
            return Ok(Shape::Turned {
                from,
                over,
                part,
                starts,
                seam: None,
                across: None,
            });
        }
        if let ([k], SurfaceKind::Turned { profile, .. }) = (&around[..], kind) {
            // One loop around the axis: the face reaches to where the
            // surface collapses, above the loop if it runs with the
            // angle, below it otherwise.
            ensure!(loops.len() == 1, "a face around a pole with holes in it");
            let above = turns[*k] == 1;
            let (a, b) = profile.domain();
            let end = if above { b } else { a };
            ensure!(
                frame.context("no axis")?.around(profile.point_at(end)).1 <= self.size * 1e-9,
                "a face around its axis with one edge"
            );
            let part = if above { (part.0, b) } else { (a, part.1) };
            return Ok(Shape::Turned {
                from: angles[*k][0][0],
                over: TAU,
                part,
                starts: vec![angles[*k][0][0]],
                seam: Some(Seam::Cap { above }),
                across: None,
            });
        }
        if let ([k], SurfaceKind::Closed { surface, .. }) = (&around[..], kind) {
            // The same on a NURBS surface that closes around and comes
            // to a point at one end.
            ensure!(loops.len() == 1, "a face around a pole with holes in it");
            let above = turns[*k] == 1;
            let ((u0, u1), (a, b)) = surface.domain();
            let end = if above { b } else { a };
            let tip = surface.point_at(u0, end);
            ensure!(
                (1..8).all(|i| {
                    let u = u0 + (u1 - u0) * f64::from(i) / 8.0;
                    surface.point_at(u, end).distance(tip) <= self.size * 1e-9
                }),
                "a face around its surface with one edge"
            );
            let part = if above { (part.0, b) } else { (a, part.1) };
            return Ok(Shape::Turned {
                from: angles[*k][0][0],
                over: TAU,
                part,
                starts: vec![angles[*k][0][0]],
                seam: Some(Seam::Cap { above }),
                across: None,
            });
        }
        let (plus, minus) = match around[..] {
            [a, b] if turns[a] == 1 && turns[b] == -1 => (a, b),
            [a, b] if turns[a] == -1 && turns[b] == 1 => (b, a),
            _ => bail!("a face around a pole of its surface"),
        };
        // The seam is where the loop that runs with the angle starts —
        // or, on a surface that is closed as it is, where that comes
        // by the surface's own seam.
        let begin = angles[plus][0][0];
        let (from, first) = if matches!(kind, SurfaceKind::Closed { .. }) {
            let seam = TAU * (begin / TAU - 1e-9).ceil();
            if seam - begin <= 1e-9 {
                (begin, None)
            } else {
                let v = self.seam_cut(kind, &loops[plus], &angles[plus], 0.0, seam, true)?;
                (seam, Some(v))
            }
        } else {
            (begin, None)
        };
        // Where the other loop comes by there: it runs down from its
        // start by a whole turn.
        let start = unwrap(angles[minus][0][0], from + TAU / 2.0);
        let start = if start <= from + 1e-9 {
            start + TAU
        } else {
            start
        };
        let shift = start - angles[minus][0][0];
        let at_start = start >= from + TAU - 1e-9;
        let vertex = if at_start {
            let (rim, reversed) = loops[minus][0];
            self.rims[rim].ends[usize::from(reversed)]
        } else {
            self.seam_cut(kind, &loops[minus], &angles[minus], shift, from, false)?
        };
        // Holes lie between the seam and itself, a turn on.
        let mut starts = vec![from; loops.len()];
        for (k, walks) in angles.iter().enumerate() {
            if k == plus || k == minus {
                continue;
            }
            let (lo, hi) = walks
                .iter()
                .flatten()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |m, a| {
                    (m.0.min(*a), m.1.max(*a))
                });
            let shift = TAU * ((from - lo) / TAU).ceil();
            ensure!(
                hi + shift <= from + TAU,
                "a hole across the seam of its face"
            );
            starts[k] = walks[0][0] + shift;
        }
        Ok(Shape::Turned {
            from,
            over: TAU,
            part,
            starts,
            seam: Some(Seam::Band {
                plus,
                minus,
                vertex,
                first,
            }),
            across: None,
        })
    }

    /// The vertex where the loop `lp` comes by the angle `seam`,
    /// rising through it or falling: an end of the edge there, or a new
    /// vertex the edge is cut at. `walks` are the angles along the
    /// loop, to be moved by `shift`.
    fn seam_cut(
        &mut self,
        kind: &SurfaceKind,
        lp: &Loop,
        walks: &[Vec<f64>],
        shift: f64,
        seam: f64,
        rising: bool,
    ) -> anyhow::Result<usize> {
        // Before the seam, the way the loop runs.
        let before = |a: f64| if rising { a <= seam } else { a >= seam };
        let mut found = None;
        'walks: for (w, walk) in walks.iter().enumerate() {
            for k in 0..walk.len() - 1 {
                let (a, b) = (walk[k] + shift, walk[k + 1] + shift);
                if before(a) && !before(b) || (before(a) && b == seam && a != b) {
                    found = Some((w, k, a));
                    break 'walks;
                }
            }
        }
        let (w, k, mut a) = found.context("a loop around an axis never comes by its seam")?;
        let (rim, reversed) = lp[w];
        // Between two of the walk's points, by halving.
        let fractions = self.fractions(rim, reversed);
        let (mut lo, mut hi) = (fractions[k], fractions[k + 1]);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            let at = self.rims[rim].at(reversed, mid);
            let angle = unwrap(kind.turn(at).context("no axis")?.0, a);
            if before(angle) {
                (lo, a) = (mid, angle);
            } else {
                hi = mid;
            }
        }
        let f = (lo + hi) / 2.0;
        let point = self.rims[rim].at(reversed, f);
        let r = &self.rims[rim];
        let ends = if reversed {
            [r.ends[1], r.ends[0]]
        } else {
            r.ends
        };
        let near = self.tol * 4.0;
        Ok(if point.distance(self.vertices[ends[0]].point) <= near {
            ends[0]
        } else if point.distance(self.vertices[ends[1]].point) <= near {
            ends[1]
        } else {
            self.vertices.push(Vertex {
                point,
                tolerance: self.tol,
                name: None,
            });
            let v = self.vertices.len() - 1;
            let f = if reversed { 1.0 - f } else { f };
            let r = &mut self.rims[rim];
            r.cuts.push((r.range.0 + (r.range.1 - r.range.0) * f, v));
            v
        })
    }

    /// Whether `p`, on the axis of a turned surface, is where its
    /// profile starts (and not where it ends).
    fn at_start(&self, kind: &SurfaceKind, p: Point3) -> bool {
        if let SurfaceKind::Closed { poles, .. } = kind {
            let off = |pole: &Option<Point3>| pole.map_or(f64::INFINITY, |q| q.distance(p));
            return off(&poles[0]) <= off(&poles[1]);
        }
        let SurfaceKind::Turned { profile, .. } = kind else {
            return true;
        };
        let (a, b) = profile.domain();
        kind.along_profile(p).is_none_or(|t| t - a < b - t)
    }

    /// Cuts the edges where seams asked for it.
    fn cut(&mut self) {
        for rim in &mut self.rims {
            rim.cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
            rim.cuts.dedup_by(|a, b| a.1 == b.1);
            let mut from = (rim.range.0, rim.ends[0]);
            let stops: Vec<(f64, usize)> = rim
                .cuts
                .iter()
                .copied()
                .chain([(rim.range.1, rim.ends[1])])
                .collect();
            for to in stops {
                rim.pieces.push(self.edges.len());
                self.edges.push(Edge {
                    curve: rim.curve.clone(),
                    range: (from.0, to.0),
                    ends: [from.1, to.1],
                    tolerance: self.tol,
                    name: None,
                    also: Vec::new(),
                });
                from = to;
            }
        }
    }

    /// The steps of `lp` over the cut edges, starting at `vertex` if one
    /// is given.
    fn walk(&self, lp: &Loop, vertex: Option<usize>) -> anyhow::Result<Vec<(usize, bool)>> {
        let mut steps = Vec::new();
        for &(rim, reversed) in lp {
            let pieces = &self.rims[rim].pieces;
            if reversed {
                steps.extend(pieces.iter().rev().map(|&e| (e, true)));
            } else {
                steps.extend(pieces.iter().map(|&e| (e, false)));
            }
        }
        if let Some(v) = vertex {
            let at = steps
                .iter()
                .position(|&(e, reversed)| self.edges[e].ends[usize::from(reversed)] == v)
                .context("a seam's vertex is not on its loop")?;
            steps.rotate_left(at);
        }
        Ok(steps)
    }

    fn face(&mut self, plan: &Plan) -> anyhow::Result<Face> {
        let patch: Patch = match &plan.shape {
            Shape::Flat(extent) => plan.kind.flat(*extent)?,
            Shape::Whole => plan.kind.whole()?,
            Shape::Turned {
                from,
                over,
                part,
                across,
                ..
            } => {
                let patch = plan.kind.turned(*from, *over, *part)?;
                match across {
                    Some(across) => patch.across(across.base),
                    None => patch,
                }
            }
        };
        let across = match &plan.shape {
            Shape::Turned { across, .. } => across.as_ref(),
            _ => None,
        };
        let surface = Arc::new(patch.surface.clone());
        let seam = match &plan.shape {
            Shape::Turned { seam, .. } => seam.as_ref(),
            _ => None,
        };
        let accuracy = self.size * 1e-9;
        let flat = Flat::of(&surface);
        let mut loops: Vec<Vec<Use>> = Vec::new();
        for (k, lp) in plan.loops.iter().enumerate() {
            let upper = matches!(seam, Some(Seam::Band { minus, .. }) if *minus == k)
                || matches!(seam, Some(Seam::Cap { above: false }));
            let rotate = match seam {
                Some(Seam::Band { minus, vertex, .. }) if *minus == k => Some(*vertex),
                Some(Seam::Band { plus, first, .. }) if *plus == k => *first,
                _ => None,
            };
            let steps = self.walk(lp, rotate)?;
            // The angle the loop starts at, and from there on without
            // jumps.
            let mut angle = match &plan.shape {
                Shape::Turned {
                    from, over, starts, ..
                } => Some(if upper { from + over } else { starts[k] }),
                _ => None,
            };
            // Around the second parameter of a surface closed both ways
            // likewise: from where the loop starts (a loop turned to
            // start at a seam's vertex: from the nearest to its middle).
            let mut other = match (across, steps.first()) {
                (Some(across), Some(&(e, reversed))) => {
                    let (start, (lo, hi)) = across.loops[k];
                    Some(if rotate.is_none() {
                        start
                    } else {
                        let edge = &self.edges[e];
                        let t = if reversed { edge.range.1 } else { edge.range.0 };
                        let raw = plan.kind.around_both(edge.point(t)).map_or(start, |a| a.1);
                        unwrap(raw, (lo + hi) / 2.0).clamp(lo, hi)
                    })
                }
                _ => None,
            };
            let mut uses = Vec::with_capacity(steps.len());
            let mut last: Option<Point2> = None;
            let mut pole: Option<bool> = None;
            let mut seen = false;
            for (e, reversed) in steps {
                let edge = &self.edges[e];
                let (a, b) = edge.range;
                let along = |f: f64| {
                    let f = if reversed { 1.0 - f } else { f };
                    a + (b - a) * f
                };
                // To the middle of the step, and on to its end.
                let mut middle = angle;
                let mut beside = other;
                if let Some(at) = angle.as_mut() {
                    for i in 0..=SAMPLES {
                        let p = edge.point(along(i as f64 / SAMPLES as f64));
                        if let Some(at) = other.as_mut()
                            && let Some((_, raw)) = plan.kind.around_both(p)
                        {
                            *at = unwrap(raw, *at);
                            if i == SAMPLES / 2 {
                                beside = Some(*at);
                            }
                        }
                        let Some((raw, rho)) = plan.kind.turn(p) else {
                            break;
                        };
                        if rho > self.size * 1e-6 {
                            if !std::mem::replace(&mut seen, true) {
                                pole = None;
                            }
                            *at = onward(*at, raw, pole.take());
                        } else {
                            pole = Some(self.at_start(&plan.kind, p));
                        }
                        if i == SAMPLES / 2 {
                            middle = Some(*at);
                        }
                    }
                }
                // On a flat face the edge's curve in (u, v) is the edge
                // itself, laid into the plane: nothing to look for.
                if let Some(flat) = &flat {
                    let c = &edge.curve;
                    let curve = Curve2::new(
                        c.degree(),
                        c.knots().to_vec(),
                        c.control_points().iter().map(|p| flat.uv(*p)).collect(),
                        Some(c.weights().to_vec()),
                    )?;
                    let off = (0..=8)
                        .map(|k| flat.off(edge.point(a + (b - a) * f64::from(k) / 8.0)))
                        .fold(0.0, f64::max);
                    ensure!(
                        off <= self.size * 1e-3,
                        "an edge of face #{} is {off:.3e} off its surface",
                        plan.id
                    );
                    last = Some(curve.point_at(along(1.0)));
                    uses.push(Use {
                        walk: Walk::Edge { edge: e, reversed },
                        pcurve: Arc::new(curve),
                    });
                    continue;
                }
                let t = along(0.5);
                let p = edge.point(t);
                let guess = match patch.guess(p, middle, beside) {
                    Some(g) => g,
                    // On a surface that does not close, where a quick
                    // look finds the point is where it is.
                    None if matches!(plan.kind, SurfaceKind::Spline(_))
                        && near(&surface, p, self.size * 1e-4).is_some() =>
                    {
                        near(&surface, p, self.size * 1e-4).unwrap_or_default()
                    }
                    None => {
                        // Anywhere on the surface: the place nearest
                        // to where the loop last was.
                        let found = locate(&surface, p, self.size * 1e-4);
                        let near = last.unwrap_or(Point2::ZERO);
                        found
                            .into_iter()
                            .min_by(|x, y| x.distance(near).total_cmp(&y.distance(near)))
                            .with_context(|| {
                                format!("an edge of face #{} is not on its surface", plan.id)
                            })?
                    }
                };
                let (uv, _) = foot(&surface, p, guess);
                let (curve, off) = match straight_on(&surface, edge, (t, uv), self.size) {
                    Some(straight) => straight,
                    None => pcurve(&surface, &edge.curve, edge.range, (t, uv), accuracy)
                        .with_context(|| {
                            format!("an edge of face #{} could not be drawn on it", plan.id)
                        })?,
                };
                ensure!(
                    off <= self.size * 1e-3,
                    "an edge of face #{} is {off:.3e} off its surface",
                    plan.id
                );
                last = Some(curve.point_at(along(1.0)));
                uses.push(Use {
                    walk: Walk::Edge { edge: e, reversed },
                    pcurve: Arc::new(curve),
                });
            }
            // Where two steps meet on the axis at different angles, the
            // loop stays at that point while its curve in (u, v) runs
            // along the collapsed side.
            let mut with_poles = Vec::with_capacity(uses.len());
            for (i, step) in uses.iter().enumerate() {
                with_poles.push(step.clone());
                let next = &uses[(i + 1) % uses.len()];
                let (
                    Walk::Edge {
                        edge: e0,
                        reversed: r0,
                    },
                    Walk::Edge {
                        edge: e1,
                        reversed: r1,
                    },
                ) = (step.walk, next.walk)
                else {
                    continue;
                };
                let (a, b) = (&self.edges[e0], &self.edges[e1]);
                let vertex = a.ends[usize::from(!r0)];
                let here = step.pcurve.point_at(if r0 { a.range.0 } else { a.range.1 });
                let there = next.pcurve.point_at(if r1 { b.range.1 } else { b.range.0 });
                let ((u0, u1), (v0, v1)) = surface.domain();
                let apart = ((here.x - there.x) / (u1 - u0)).hypot((here.y - there.y) / (v1 - v0));
                if apart <= 1e-9 {
                    continue;
                }
                // Collapsed: every point between the two in (u, v) is
                // the one point in space.
                let at = self.vertices[vertex].point;
                let near = (self.size * 1e-6).max(self.tol * 16.0);
                let collapsed = [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().all(|f| {
                    let q = here.lerp(there, f);
                    surface.point_at(q.x, q.y).distance(at) <= near
                });
                if collapsed {
                    with_poles.push(Use {
                        walk: Walk::Pole {
                            vertex,
                            range: (0.0, 1.0),
                        },
                        pcurve: Arc::new(Curve2::line(here, there)?),
                    });
                }
            }
            loops.push(with_poles);
        }
        match seam {
            Some(Seam::Band { plus, minus, .. }) => {
                let upper = std::mem::take(&mut loops[*minus]);
                let lower = std::mem::take(&mut loops[*plus]);
                let sewn = self.sew(&surface, Some(lower), Some(upper))?;
                loops.retain(|l| !l.is_empty());
                loops.insert(0, sewn);
            }
            Some(Seam::Cap { above: true }) => {
                let lower = std::mem::take(&mut loops[0]);
                loops = vec![self.sew(&surface, Some(lower), None)?];
            }
            Some(Seam::Cap { above: false }) => {
                let upper = std::mem::take(&mut loops[0]);
                loops = vec![self.sew(&surface, None, Some(upper))?];
            }
            Some(Seam::Globe) => loops = vec![self.sew(&surface, None, None)?],
            None => {}
        }
        // A face lies to the left of its outline as that runs on the
        // surface. Files that give one oriented edge to both faces
        // along it (against the rule, but they exist) have one of the
        // two the wrong way around: its outline runs clockwise, and is
        // turned.
        // (Its area by Green's theorem along the curves themselves:
        // chords would not tell which way a sliver's outline runs.)
        let turning = |lp: &Vec<Use>| -> f64 {
            let ends = |step: &Use| match step.walk {
                Walk::Edge { edge, reversed } => {
                    let (a, b) = self.edges[edge].range;
                    if reversed { (b, a) } else { (a, b) }
                }
                Walk::Pole { range, .. } => range,
            };
            let mut sum = 0.0;
            for (k, step) in lp.iter().enumerate() {
                let next = &lp[(k + 1) % lp.len()];
                let ((a, b), (onward, _)) = (ends(step), ends(next));
                sum += workpiece_boolean::geometry::swept(&step.pcurve, a, b)
                    + step
                        .pcurve
                        .point_at(b)
                        .perp_dot(next.pcurve.point_at(onward))
                        / 2.0;
            }
            sum
        };
        let outline = loops
            .iter()
            .map(turning)
            .max_by(|a, b| a.abs().total_cmp(&b.abs()));
        if outline.is_some_and(|area| area < 0.0) {
            for lp in &mut loops {
                lp.reverse();
                for step in lp.iter_mut() {
                    match &mut step.walk {
                        Walk::Edge { reversed, .. } => *reversed = !*reversed,
                        Walk::Pole { .. } => step.pcurve = Arc::new(step.pcurve.reversed()),
                    }
                }
            }
        }
        // A face is some of its surface. (A seam walked there and back
        // at one place of it, with nothing else, bounds nothing.)
        let ((u0, u1), (v0, v1)) = surface.domain();
        let covered: f64 = loops.iter().map(turning).sum();
        ensure!(
            covered.abs() > 1e-14 * (u1 - u0) * (v1 - v0),
            "the loops of face #{} bound nothing of its surface",
            plan.id
        );
        // Each step ends where the next starts, on the surface too.
        for lp in &loops {
            for (k, step) in lp.iter().enumerate() {
                let next = &lp[(k + 1) % lp.len()];
                let end = |u: &Use, last: bool| match u.walk {
                    Walk::Edge { edge, reversed } => {
                        let (a, b) = self.edges[edge].range;
                        u.pcurve.point_at(if reversed != last { b } else { a })
                    }
                    Walk::Pole { range, .. } => {
                        u.pcurve.point_at(if last { range.1 } else { range.0 })
                    }
                };
                let (here, there) = (end(step, true), end(next, false));
                let gap = surface
                    .point_at(here.x, here.y)
                    .distance(surface.point_at(there.x, there.y));
                let ((u0, u1), (v0, v1)) = surface.domain();
                let apart = ((here.x - there.x) / (u1 - u0)).hypot((here.y - there.y) / (v1 - v0));
                ensure!(
                    // (A small face of a file whose edges are off
                    // its surface is a good part of its surface
                    // across; a step across a seam is all of it.)
                    gap <= self.size * 2e-3
                        && (apart <= 0.05 || (gap <= self.size * 1e-4 && apart <= 0.4)),
                    "a loop of face #{} does not close on its surface: {gap:.3e} apart, \
                     {apart:.3} of the surface in (u, v)",
                    plan.id
                );
            }
        }
        Ok(Face {
            surface,
            outward: plan.outward,
            loops,
            tolerance: self.tol,
            name: None,
            also: Vec::new(),
            smooth: Vec::new(),
        })
    }

    /// Makes one loop of a face that goes around its axis: along its
    /// lower edge with the angle, up the seam, back along its upper
    /// edge, and down the seam again. Where an edge is missing the
    /// surface collapses to a point there, and the loop stays at it.
    fn sew(
        &mut self,
        surface: &Arc<workpiece_core::world::topo::NurbsSurface>,
        lower: Option<Vec<Use>>,
        upper: Option<Vec<Use>>,
    ) -> anyhow::Result<Vec<Use>> {
        let ((u0, u1), (v0, v1)) = surface.domain();
        // Where a loop starts: its vertex and its height on the
        // surface.
        let start = |b: &Self, lp: &[Use]| -> anyhow::Result<(usize, f64)> {
            let first = lp.first().context("an empty loop")?;
            let Walk::Edge { edge, reversed } = first.walk else {
                bail!("a loop that starts at a pole");
            };
            let e = &b.edges[edge];
            let t = if reversed { e.range.1 } else { e.range.0 };
            Ok((e.ends[usize::from(reversed)], first.pcurve.point_at(t).y))
        };
        let pole = |b: &mut Self, v: f64| {
            b.vertices.push(Vertex {
                point: surface.point_at(u0, v),
                tolerance: b.tol,
                name: None,
            });
            (b.vertices.len() - 1, v)
        };
        let (low, low_v) = match &lower {
            Some(lp) => start(self, lp)?,
            None => pole(self, v0),
        };
        let (high, high_v) = match &upper {
            Some(lp) => start(self, lp)?,
            None => pole(self, v1),
        };
        ensure!(low_v < high_v, "a face around an axis upside down");
        // The seam: the surface's first line across, between the two.
        let (_, nv) = surface.counts();
        let line = Curve3::new(
            surface.degrees().1,
            surface.knots().1.to_vec(),
            (0..nv).map(|j| surface.control_point(0, j)).collect(),
            Some(surface.weights()[..nv].to_vec()),
        )?;
        let edge = self.edges.len();
        self.edges.push(Edge {
            curve: Arc::new(line),
            range: (low_v, high_v),
            ends: [low, high],
            tolerance: self.tol,
            name: None,
            also: Vec::new(),
        });
        let line = |a: Point2, b: Point2, range: (f64, f64)| -> anyhow::Result<Arc<Curve2>> {
            Ok(Arc::new(Curve2::new(
                1,
                vec![range.0, range.0, range.1, range.1],
                vec![a, b],
                None,
            )?))
        };
        let side = |u: f64| {
            line(
                Point2::new(u, low_v),
                Point2::new(u, high_v),
                (low_v, high_v),
            )
        };
        let around = |vertex: usize, from: Point2, to: Point2| -> anyhow::Result<Use> {
            Ok(Use {
                walk: Walk::Pole {
                    vertex,
                    range: (0.0, 1.0),
                },
                pcurve: line(from, to, (0.0, 1.0))?,
            })
        };
        let mut all = match lower {
            Some(lp) => lp,
            None => vec![around(low, Point2::new(u0, v0), Point2::new(u1, v0))?],
        };
        all.push(Use {
            walk: Walk::Edge {
                edge,
                reversed: false,
            },
            pcurve: side(u1)?,
        });
        match upper {
            Some(lp) => all.extend(lp),
            None => all.push(around(high, Point2::new(u1, v1), Point2::new(u0, v1))?),
        }
        all.push(Use {
            walk: Walk::Edge {
                edge,
                reversed: true,
            },
            pcurve: side(u0)?,
        });
        Ok(all)
    }
}

/// A surface that is a flat parallelogram, as its (u, v) of a point.
struct Flat {
    origin: Point3,
    /// Along u and along v, each scaled so that its product with a
    /// point's offset is the point's share of that side — for sides
    /// that are square to each other, and else with the other's part
    /// taken out first.
    sides: [Point3; 2],
    normal: Point3,
    from: Point2,
    over: Point2,
}

impl Flat {
    fn of(surface: &NurbsSurface) -> Option<Self> {
        if surface.degrees() != (1, 1) || surface.counts() != (2, 2) {
            return None;
        }
        if surface.weights().iter().any(|w| (*w - 1.0).abs() > 1e-15) {
            return None;
        }
        let [p00, p01, p10, p11] = <[Point3; 4]>::try_from(surface.control_points()).ok()?;
        let (a, b) = (p10 - p00, p01 - p00);
        let size = a.length().max(b.length());
        if (p11 - (p10 + p01 - p00)).length() > 1e-12 * size {
            return None;
        }
        let normal = a.cross(b);
        if normal.length() <= 1e-12 * size * size {
            return None;
        }
        // The dual of the two sides in their plane.
        let n2 = normal.length_squared();
        let sides = [b.cross(normal) / n2, normal.cross(a) / n2];
        let ((u0, u1), (v0, v1)) = surface.domain();
        Some(Flat {
            origin: p00,
            sides,
            normal: normal.normalize(),
            from: Point2::new(u0, v0),
            over: Point2::new(u1 - u0, v1 - v0),
        })
    }

    fn uv(&self, p: Point3) -> Point2 {
        let d = p - self.origin;
        self.from + Point2::new(d.dot(self.sides[0]), d.dot(self.sides[1])) * self.over
    }

    /// How far `p` is from the plane.
    fn off(&self, p: Point3) -> f64 {
        (p - self.origin).dot(self.normal).abs()
    }
}

/// The curve of `edge` in the (u, v) of `surface` where that is a
/// straight line run through evenly as the edge is — a ruling of a
/// cylinder or a cone, a rim laid out as the surface is around: through
/// `seed` (a parameter of the edge and its (u, v)), found from two more
/// places and held against the edge all along it. With how far its
/// image strays from the edge; `None` where the edge is no such line.
fn straight_on(
    surface: &NurbsSurface,
    edge: &workpiece_boolean::model::Edge,
    seed: (f64, Point2),
    size: f64,
) -> Option<(Curve2, f64)> {
    let (a, b) = edge.range;
    if !(b - a).is_finite() || b == a {
        return None;
    }
    let (t, uv) = seed;
    let quarter = (b - a) / 4.0;
    let (t0, t1) = (t - quarter, t + quarter);
    let (uv0, off0) = foot(surface, edge.point(t0), uv);
    let (uv1, off1) = foot(surface, edge.point(t1), uv);
    let fine = size * 1e-12;
    if off0 > fine || off1 > fine {
        return None;
    }
    let slope = (uv1 - uv0) / (t1 - t0);
    let at = |s: f64| uv + slope * (s - t);
    // (Not past the surface's own parameters: a line that leaves them
    // is one across a seam, which is drawn otherwise.)
    let ((u0, u1), (v0, v1)) = surface.domain();
    let slack = Point2::new(u1 - u0, v1 - v0) * 1e-12;
    let mut off = 0.0_f64;
    for k in 0..=8 {
        let s = a + (b - a) * f64::from(k) / 8.0;
        let q = at(s);
        if q.x < u0 - slack.x || q.x > u1 + slack.x || q.y < v0 - slack.y || q.y > v1 + slack.y {
            return None;
        }
        off = off.max(surface.point_at(q.x, q.y).distance(edge.point(s)));
        if off > fine {
            return None;
        }
    }
    let clamp = |q: Point2| Point2::new(q.x.clamp(u0, u1), q.y.clamp(v0, v1));
    let line = Curve2::new(1, vec![a, a, b, b], vec![clamp(at(a)), clamp(at(b))], None).ok()?;
    Some((line, off))
}

/// The faces of the closed shell `#id`, each with whether the shell is
/// used inside out.
fn shell_faces(
    r: &Reader<'_>,
    id: u32,
    flipped: bool,
    out: &mut Vec<(u32, bool)>,
) -> anyhow::Result<()> {
    let shell = r.get(id)?;
    if shell.name() == "ORIENTED_CLOSED_SHELL" {
        let args = shell.own();
        let inner = args.reference(2).context("no shell")?;
        return shell_faces(r, inner, flipped != !args.flag(3).unwrap_or(true), out);
    }
    let faces = shell.own().list(1).context("a shell has no faces")?;
    out.extend(
        faces
            .values()
            .filter_map(|v| v.reference())
            .map(|f| (f, flipped)),
    );
    Ok(())
}

fn body(r: &Reader<'_>, root: u32) -> anyhow::Result<Solid> {
    let solid = faces_of(r, root)?;
    // A solid's edges are each walked twice, once each way.
    let mut walked: Vec<Vec<bool>> = vec![Vec::new(); solid.edges.len()];
    for face in &solid.faces {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, reversed } = step.walk {
                walked[edge].push(reversed == face.outward);
            }
        }
    }
    let open = walked
        .iter()
        .filter(|w| !w.is_empty() && (w.len() != 2 || w[0] == w[1]))
        .count();
    ensure!(open == 0, "{open} of its edges do not join two faces");
    let all: Vec<usize> = (0..solid.faces.len()).collect();
    let mut solid = solid.part(&all);

    // What the file's edges are off its surfaces is their tolerance.
    solid.settle();
    Ok(solid)
}

/// The faces of the solid or surface model `#root` as the file joins
/// them: closed or not.
fn faces_of(r: &Reader<'_>, root: u32) -> anyhow::Result<Solid> {
    let instance = r.get(root)?;
    let mut faces = Vec::new();
    let outer = if instance.parts().count() > 1 {
        instance
            .arguments("MANIFOLD_SOLID_BREP")
            .and_then(|a| a.reference(0))
    } else {
        instance.own().reference(1)
    };
    if instance.name() == "SHELL_BASED_SURFACE_MODEL" {
        // Shells that are written as surfaces: a solid if together
        // they close, as is checked of every solid.
        let shells = instance
            .own()
            .list(1)
            .context("a surface model has no shells")?;
        for shell in shells.values().filter_map(|v| v.reference()) {
            shell_faces(r, shell, false, &mut faces)?;
        }
    } else {
        shell_faces(r, outer.context("a solid has no shell")?, false, &mut faces)?;
    }
    let voids = if instance.parts().count() > 1 {
        instance
            .arguments("BREP_WITH_VOIDS")
            .and_then(|a| a.list(0))
    } else if instance.name() == "BREP_WITH_VOIDS" {
        instance.own().list(2)
    } else {
        None
    };
    for hollow in voids
        .iter()
        .flat_map(|l| l.values())
        .filter_map(|v| v.reference())
    {
        shell_faces(r, hollow, false, &mut faces)?;
    }
    let mut builder = Builder {
        r,
        corners: Vec::new(),
        size: 1.0,
        tol: 1e-7,
        vertices: Vec::new(),
        vmap: HashMap::new(),
        rims: Vec::new(),
        rmap: HashMap::new(),
        places: HashMap::new(),
        edges: Vec::new(),
    };
    // The size of the body, from its vertices: every edge of every
    // face, read once.
    let mut beyond: Vec<Point3> = Vec::new();
    for &(face, _) in &faces {
        let args = r.get(face)?.own();
        let bounds = args.list(1).context("a face has no bounds")?;
        for bound in bounds.values().filter_map(|v| v.reference()) {
            let walk = r.get(r.get(bound)?.own().reference(1).context("no loop")?)?;
            if walk.name() != "EDGE_LOOP" {
                continue;
            }
            let Some(Value::List(edges)) = walk.own().at(1) else {
                continue;
            };
            for oriented in edges.values().filter_map(|v| v.reference()) {
                let edge = r.get(r.get(oriented)?.own().reference(3).context("no edge")?)?;
                for end in [1, 2] {
                    builder.vertex(edge.own().reference(end).context("no vertex")?)?;
                }
                // An edge that closes on itself reaches beyond its one
                // vertex: as far as its curve does.
                let own = edge.own();
                if own.reference(1) == own.reference(2)
                    && let Some(curve) = own.reference(3)
                {
                    match r.curve(curve) {
                        Ok(CurveKind::Ellipse { frame, a, b }) => {
                            // Points of it, all around.
                            for k in 0..16 {
                                let (y, x) = (f64::from(k) * TAU / 16.0).sin_cos();
                                beyond.push(frame.o + frame.x * (a * x) + frame.y * (b * y));
                            }
                        }
                        Ok(CurveKind::Spline(curve)) => {
                            let (lo, hi) = curve.domain();
                            beyond.extend(
                                (0..=16)
                                    .map(|k| curve.point_at(lo + (hi - lo) * f64::from(k) / 16.0)),
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let (mut lo, mut hi) = (Point3::INFINITY, Point3::NEG_INFINITY);
    for v in &builder.vertices {
        (lo, hi) = (lo.min(v.point), hi.max(v.point));
    }
    builder.size = if builder.vertices.is_empty() {
        1.0
    } else {
        (hi - lo).length().max(1e-6)
    };
    // The box endless surfaces are cut down to holds the closed edges
    // too; the size, and with it every tolerance, is the vertices' —
    // unless they say next to nothing of it (a ball with one vertex).
    for p in beyond.into_iter().filter(|p| p.is_finite()) {
        (lo, hi) = (lo.min(p), hi.max(p));
    }
    let across = (hi - lo).length();
    if across.is_finite() && across > builder.size * 2.0 && !builder.vertices.is_empty() {
        builder.size = across;
    }
    builder.corners = (0..8)
        .map(|k| {
            let pick = |bit: usize, a: f64, b: f64| if k >> bit & 1 == 0 { a } else { b };
            Point3::new(
                pick(0, lo.x, hi.x),
                pick(1, lo.y, hi.y),
                pick(2, lo.z, hi.z),
            )
        })
        .collect();
    builder.tol = (builder.size * 1e-9).max(1e-7);
    for v in &mut builder.vertices {
        v.tolerance = builder.tol;
    }
    let plans: Vec<Plan> = faces
        .iter()
        .map(|&(face, flipped)| {
            builder
                .plan(face, flipped)
                .with_context(|| format!("face #{face}"))
        })
        .collect::<Result<_, _>>()?;
    builder.cut();
    let mut solid = Solid::default();
    for plan in &plans {
        let face = builder
            .face(plan)
            .with_context(|| format!("face #{}", plan.id))?;
        solid.faces.push(face);
    }
    solid.vertices = builder.vertices;
    solid.edges = builder.edges;
    // (Parts that touch along an edge, written as one solid, each get
    // that edge for themselves.)
    solid.part_edges();
    Ok(solid)
}

/// Reads the surface models of the STEP file `bytes`: each as a sheet
/// of the faces the file gives it, joined only where the file joins
/// them. What [`workpiece_boolean::stitched`] sews into a solid.
pub fn import_sheets(bytes: &[u8]) -> anyhow::Result<Vec<Solid>> {
    let file = File::new(bytes)?;
    let reader = Reader::new(&file);
    file.of_type("SHELL_BASED_SURFACE_MODEL")
        .map(|root| {
            let mut sheet = faces_of(&reader, root.id)
                .with_context(|| format!("surface model #{}", root.id))?;
            sheet.settle();
            Ok(sheet)
        })
        .collect()
}

/// The name of the product a solid belongs to, as far as the file says.
fn name_of(file: &File<'_>, root: u32) -> String {
    let own = file
        .get(root)
        .and_then(
            |i| match i.arguments("REPRESENTATION_ITEM").or(Some(i.own()))?.at(0) {
                Some(Value::Str(s)) => Some(s.to_owned()),
                _ => None,
            },
        )
        .unwrap_or_default();
    own.trim().to_owned()
}

/// Reads the solids of the STEP file `bytes`.
pub fn import(bytes: &[u8]) -> anyhow::Result<Imported> {
    let file = File::new(bytes)?;
    let reader = Reader::new(&file);
    let mut out = Imported {
        bodies: Vec::new(),
        refused: Vec::new(),
    };
    let roots = file.instances().filter(|i| {
        i.is("MANIFOLD_SOLID_BREP") || i.is("BREP_WITH_VOIDS") || i.is("SHELL_BASED_SURFACE_MODEL")
    });
    let assembly = Assembly::new(&reader);
    let mut exact = false;
    for root in roots {
        exact = true;
        // A solid is only what stands as one: written to a topology of
        // its own and checked there, so that a file's faults are told
        // here and not handed on.
        let checked = body(&reader, root.id).and_then(|solid| {
            // Each loop ends where it starts, step after step.
            for (f, face) in solid.faces.iter().enumerate() {
                for lp in &face.loops {
                    for (k, step) in lp.iter().enumerate() {
                        let next = &lp[(k + 1) % lp.len()];
                        ensure!(
                            solid.ends(step).1 == solid.ends(next).0,
                            "its face {}: a loop of {} edges does not go on after its edge {}",
                            f + 1,
                            lp.len(),
                            k + 1
                        );
                    }
                }
            }
            let mut topo = workpiece_core::world::topo::Topo::new();
            workpiece_boolean::write(&mut topo, &solid, "solid")
                .map_err(|e| anyhow::anyhow!("its faces do not make a solid: {e}"))?;
            if let Err(faults) = topo.validate() {
                let first = faults.first().map(ToString::to_string).unwrap_or_default();
                bail!(
                    "it is not valid as it is written ({} faults): {first}",
                    faults.len()
                );
            }
            Ok(solid)
        });
        let solid = match checked {
            Ok(solid) => solid,
            // Surfaces that do not close into a solid are no solid
            // that was refused: they are surfaces, which are not read.
            Err(_) if root.is("SHELL_BASED_SURFACE_MODEL") => continue,
            Err(why) => {
                out.refused.push(format!("solid #{}: {why:#}", root.id));
                continue;
            }
        };
        // Once for every place the file puts it.
        for place in assembly.places(root.id) {
            let solid = if place.is_none() {
                solid.clone()
            } else {
                match solid.moved(&|p| place.of(p)) {
                    Ok(moved) => moved,
                    Err(why) => {
                        out.refused.push(format!("solid #{}: {why}", root.id));
                        continue;
                    }
                }
            };
            out.bodies.push(Body {
                name: name_of(&file, root.id),
                solid,
            });
        }
    }
    // A file without exact solids may hold them as triangles (AP242).
    // Beside exact solids those are pictures of the same, and not read.
    let meshes = file
        .instances()
        .filter(|i| !exact && (i.is("TESSELLATED_SOLID") || i.is("TESSELLATED_SHELL")));
    for root in meshes {
        let solid = match crate::mesh::tessellated(&reader, root.id) {
            Ok(solid) => solid,
            // Triangles that do not close are a surface.
            Err(_) if root.is("TESSELLATED_SHELL") => continue,
            Err(why) => {
                out.refused.push(format!("solid #{}: {why:#}", root.id));
                continue;
            }
        };
        for place in assembly.places(root.id) {
            let moved = match solid.moved(&|p| place.of(p)) {
                Ok(moved) => moved,
                Err(why) => {
                    out.refused.push(format!("solid #{}: {why}", root.id));
                    continue;
                }
            };
            out.bodies.push(Body {
                name: name_of(&file, root.id),
                solid: moved,
            });
        }
    }
    Ok(out)
}
