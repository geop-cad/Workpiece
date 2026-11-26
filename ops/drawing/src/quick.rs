//! A drawing of the model: what is seen of it from the front, from
//! above and from the right — its edges and the outlines of its round
//! faces, those hidden behind it apart from those in view — and a
//! section through its middle; with the views' sizes, the diameters of
//! its round holes and bosses, and a title block. As a DXF or a PDF
//! file.

use std::fmt::Write;
use workpiece_core::math::{angle_between, from_angle};

use workpiece_core::Unit;
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::World;
use workpiece_core::world::topo::BodyKind;
use workpiece_rasterize::tessellate;

/// One view of a drawing.
pub struct View {
    /// What it is called: `FRONT`, `TOP`, `RIGHT`.
    pub name: &'static str,
    /// Its lines on the sheet, each with whether the model hides it.
    pub lines: Vec<(Point2, Point2, bool)>,
    /// Where on the sheet its lower left corner is, and how big it is.
    pub corner: Point2,
    /// Its width and height.
    pub size: Point2,
    /// What is written in it: the diameters of what is round, each at
    /// its middle on the sheet.
    pub notes: Vec<(Point2, String)>,
}

/// A model drawn in three views, laid out as a drawing is in
/// third-angle projection: the view from above over the front view,
/// the view from the right beside it.
pub struct ThreeViews {
    /// The views.
    pub views: Vec<View>,
    /// How big the sheet is that holds them.
    pub size: Point2,
    /// What it is a drawing of.
    pub title: String,
    /// Where its section cuts: square to an axis, and how far along the
    /// model (0 to 1).
    pub section: (workpiece_core::math::Axis, f64),
    /// The dimensions the part's sketches are held to, with their
    /// tolerances: of each sketch its name and what its dimensions say.
    /// Written as a table beside the title.
    pub dimensions: Vec<(String, Vec<String>)>,
}

/// What of a model is drawn: runs of points along its edges, and the
/// triangles of its faces with the normals at their corners.
#[derive(Default)]
pub struct Shape {
    /// Runs of points along the edges.
    pub runs: Vec<Vec<Point3>>,
    /// Of each run, whether its edge is smooth: the faces on its two
    /// sides go on into each other there without a kink (where a
    /// fillet meets what it rounds) — a tangent edge.
    pub smooth: Vec<bool>,
    /// Of each run, the outward normals of the faces on its two sides
    /// at its middle (nothing where it has not two).
    pub sides: Vec<[Point3; 2]>,
    /// Of each run, at each of its points, the way out of the body
    /// there: between the outward normals of the faces on its sides
    /// (nothing where that is not known).
    pub outward: Vec<Vec<Point3>>,
    /// The edges that are arcs of circles: middle, axis, radius, and
    /// the angle they go around.
    pub circles: Vec<(Point3, Point3, f64, f64)>,
    /// The triangles of the faces, and the normals at their corners.
    pub triangles: Vec<([Point3; 3], [Point3; 3])>,
    /// Of each triangle, the body it is of: the how manyth of them
    /// `shape` met.
    pub owners: Vec<usize>,
    /// How many bodies there are.
    pub solids: usize,
    /// Of each body, how a section treats the part it is of, and where
    /// that part is.
    pub sectioned: Vec<(workpiece_core::world::Sectioned, workpiece_core::Pose)>,
}

/// The solid bodies of `world` (and of what is placed in it, where
/// `pose` puts it), as what is drawn of them.
pub fn shape(world: &World, pose: workpiece_core::Pose, out: &mut Shape) {
    let topo = &world.topo;
    for (id, body) in topo.bodies().iter() {
        if body.kind != BodyKind::Solid {
            continue;
        }
        let Ok(solid) = workpiece_boolean::read(topo, id) else {
            continue;
        };
        // (A seam, where a face meets itself — the edge from the middle
        // of a disc to its rim — is no edge of the drawing.)
        let mut sides: Vec<Vec<usize>> = vec![Vec::new(); solid.edges.len()];
        // (Of each edge, the outward normals of the faces on its sides
        // at its middle.)
        let mut normals: Vec<Vec<Point3>> = vec![Vec::new(); solid.edges.len()];
        let mut bends: Vec<Vec<f64>> = vec![Vec::new(); solid.edges.len()];
        // (How many pieces each edge is drawn of: 48, more of one that
        // turns much — a large circle — so that it is drawn round, not
        // as a polygon: no piece turns by more than about 2°.)
        let steps: Vec<usize> = solid
            .edges
            .iter()
            .map(|edge| {
                let (from, to) = edge.range;
                let at = |k: usize| edge.curve.point_at(from + (to - from) * k as f64 / 48.0);
                let turn: f64 = (1..48)
                    .map(|k| {
                        let (a, b, c) = (at(k - 1), at(k), at(k + 1));
                        let (u, v) = (b - a, c - b);
                        if u.length() < 1e-300 || v.length() < 1e-300 {
                            0.0
                        } else {
                            angle_between(u, v)
                        }
                    })
                    .sum();
                ((turn / 0.035).ceil() as usize).clamp(48, 240)
            })
            .collect();
        // (And the sum of them at each of its points.)
        let mut outs: Vec<Vec<Point3>> = steps.iter().map(|n| vec![Point3::ZERO; n + 1]).collect();
        for (f, face) in solid.faces.iter().enumerate() {
            for step in face.loops.iter().flatten() {
                if let workpiece_boolean::model::Walk::Edge { edge, .. } = step.walk {
                    sides[edge].push(f);
                    let normal = |t: f64| {
                        let uv = step.pcurve.point_at(t);
                        let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
                        let n = du.cross(dv).normalize_or_zero();
                        if face.outward { n } else { -n }
                    };
                    normals[edge].push(normal(solid.edges[edge].middle()));
                    // How much the face bends square to the edge, into
                    // it from its middle: the change of its normal over
                    // a short step (signed: + where it bends away from
                    // its outside).
                    let reversed = matches!(
                        step.walk,
                        workpiece_boolean::model::Walk::Edge { reversed: true, .. }
                    );
                    let t = solid.edges[edge].middle();
                    let (uv, along) = step.pcurve.derivative_at(t);
                    let inward = if reversed {
                        -along.perp()
                    } else {
                        along.perp()
                    };
                    let ((u0, u1), (v0, v1)) = face.surface.domain();
                    let size = (u1 - u0).abs().max((v1 - v0).abs()).max(1e-12);
                    let inside = uv + inward.normalize_or_zero() * (size * 1e-3);
                    let (p, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
                    let (q, eu, ev) = face.surface.derivatives_at(inside.x, inside.y);
                    let (n, m) = (
                        du.cross(dv).normalize_or_zero(),
                        eu.cross(ev).normalize_or_zero(),
                    );
                    let step3 = q - p;
                    let bend = match step3.length() > 1e-15 {
                        true => (m - n).dot(step3) / step3.length_squared(),
                        false => 0.0,
                    };
                    bends[edge].push(if face.outward { bend } else { -bend });
                    let (from, to) = solid.edges[edge].range;
                    for (k, out) in outs[edge].iter_mut().enumerate() {
                        *out += normal(from + (to - from) * k as f64 / steps[edge] as f64);
                    }
                }
            }
        }
        for ((((edge, sides), normals), outs), bends) in solid
            .edges
            .iter()
            .zip(&sides)
            .zip(&normals)
            .zip(&outs)
            .zip(&bends)
        {
            if let [a, b] = sides[..]
                && a == b
            {
                continue;
            }
            // (Nor where one surface goes on across it, as smooth and
            // bending as much on both sides: a tube made of two halves,
            // a round that goes on into a round as large.)
            if let ([m, n], [j, k]) = (&normals[..], &bends[..])
                && m.dot(*n) > 0.99985
                && (j - k).abs() <= 1e-3 * j.abs().max(k.abs()).max(1e-9)
            {
                continue;
            }
            let (from, to) = edge.range;
            let n = outs.len() - 1;
            let run = (0..=n).map(|k| {
                let t = from + (to - from) * k as f64 / n as f64;
                pose.transform_point(edge.curve.point_at(t))
            });
            let run: Vec<Point3> = run.collect();
            // An arc of a circle: flat, and as far from one middle all
            // along. With how far around it goes.
            let (a, b, c) = (run[0], run[n / 3], run[2 * n / 3]);
            let normal = (b - a).cross(c - a);
            let area = normal.length_squared();
            if area > 1e-18 * a.distance(b).max(a.distance(c)).powi(4) {
                let (ab, ac) = (b - a, c - a);
                let middle = a
                    + (normal.cross(ab) * ac.length_squared()
                        + ac.cross(normal) * ab.length_squared())
                        / (2.0 * area);
                let radius = middle.distance(a);
                let axis = normal.normalize();
                let round = run.iter().all(|p| {
                    let d = *p - middle;
                    (d.length() - radius).abs() <= radius * 1e-7
                        && d.dot(axis).abs() <= radius * 1e-7
                });
                if round {
                    let around: f64 = run
                        .windows(2)
                        .map(|w| angle_between(w[0] - middle, w[1] - middle))
                        .sum();
                    out.circles.push((middle, axis, radius, around));
                }
            }
            out.runs.push(run);
            // (Within a degree of each other.)
            let smooth = match normals[..] {
                [m, n] => m != Point3::ZERO && n != Point3::ZERO && m.dot(n) > 0.99985,
                _ => false,
            };
            out.smooth.push(smooth);
            out.sides.push(match normals[..] {
                [m, n] => [pose.transform_vector(m), pose.transform_vector(n)],
                _ => [Point3::ZERO; 2],
            });
            out.outward.push(
                outs.iter()
                    .map(|n| pose.transform_vector(*n).normalize_or_zero())
                    .collect(),
            );
        }
        let owner = out.solids;
        out.solids += 1;
        out.sectioned.push((world.sectioned, pose));
        // (Each face as the body uses it: its normals out of the body,
        // where the body has its surface the other way around — a round
        // made inside out, whose outlines are else looked for inside
        // it, and hidden by it.)
        let uses = body
            .lumps
            .iter()
            .filter_map(|&l| topo.lump(l))
            .flat_map(|l| &l.shells)
            .filter_map(|&s| topo.shell(s))
            .flat_map(|s| &s.faces);
        let mut done = Vec::new();
        for face_use in uses {
            if done.contains(&face_use.face) {
                continue;
            }
            done.push(face_use.face);
            let Some(mesh) = tessellate::face(topo, face_use.face) else {
                continue;
            };
            let forward = matches!(
                face_use.orientation,
                workpiece_core::world::topo::Orientation::Forward
            );
            out.triangles.extend(mesh.triangles.iter().map(|(p, n)| {
                let (p, n) = if forward {
                    (*p, *n)
                } else {
                    ([p[0], p[2], p[1]], [-n[0], -n[2], -n[1]])
                };
                (
                    p.map(|p| pose.transform_point(p)),
                    n.map(|n| pose.transform_vector(n)),
                )
            }));
            out.owners.resize(out.triangles.len(), owner);
        }
    }
    for instance in world.instances.values() {
        shape(&instance.world, pose * instance.pose, out);
    }
}

/// A number as a drawing writes it.
fn number(x: f64) -> String {
    let text = format!("{x:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Whether something of `triangles` is between `from` and whoever looks
/// at it from far along `towards`.
pub fn hidden(
    triangles: &[([Point3; 3], [Point3; 3])],
    from: Point3,
    towards: Point3,
    near: f64,
) -> bool {
    triangles.iter().any(|([a, b, c], _)| {
        let (e1, e2) = (*b - *a, *c - *a);
        let p = towards.cross(e2);
        let det = e1.dot(p);
        // (Seen edge on, or as good as — a face that grazes the view,
        // where whether it is in front is the noise of its tilt — a
        // triangle hides nothing.)
        if det.abs() <= 1e-6 * e1.length() * e2.length() {
            return false;
        }
        let s = from - *a;
        let u = s.dot(p) / det;
        let q = s.cross(e1);
        let v = towards.dot(q) / det;
        u >= 0.0 && v >= 0.0 && u + v <= 1.0 && e2.dot(q) / det > near
    })
}

impl ThreeViews {
    /// `world` drawn, as a drawing called `title`, its section square
    /// to the axis and as far along the model (0 to 1) as `section`
    /// says — through the middle, seen from the front, if it says
    /// nothing.
    pub fn of(
        world: &World,
        title: &str,
        section: Option<(workpiece_core::math::Axis, f64)>,
    ) -> anyhow::Result<Self> {
        use workpiece_core::math::Axis;
        let mut shape_of = Shape::default();
        shape(world, workpiece_core::Pose::IDENTITY, &mut shape_of);
        anyhow::ensure!(!shape_of.runs.is_empty(), "there is no solid body to draw");
        let all = shape_of.runs.iter().flatten();
        let (lo, hi) = all.fold(
            (
                Point3::splat(f64::INFINITY),
                Point3::splat(f64::NEG_INFINITY),
            ),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let size = hi - lo;
        let reach = size.max_element();
        let gap = reach * 0.25;
        // Each view: who looks (from far along that way), where a point
        // of the model is in it, and where it is on the sheet.
        type Place = Box<dyn Fn(Point3) -> Point2>;
        // The model with what is before its middle cut away: what the
        // section shows, seen from the front.
        let (axis, along) = section.unwrap_or((Axis::Y, 0.5));
        let through = lo + (hi - lo) * along.clamp(0.0, 1.0);
        let far = size.length() * 2.0 + lo.length() + hi.length();
        // (Seen as the view along that axis sees the model: from the
        // right, from the front, from above.)
        let (looking, seen, extent): (Point3, Place, Point2) = match axis {
            Axis::X => (
                Point3::X,
                Box::new(move |p| Point2::new(p.y - lo.y, p.z - lo.z)),
                Point2::new(size.y, size.z),
            ),
            Axis::Y => (
                -Point3::Y,
                Box::new(move |p| Point2::new(p.x - lo.x, p.z - lo.z)),
                Point2::new(size.x, size.z),
            ),
            Axis::Z => (
                Point3::Z,
                Box::new(move |p| Point2::new(p.x - lo.x, p.y - lo.y)),
                Point2::new(size.x, size.y),
            ),
        };
        let opened = workpiece_shape::cut::cut(world, through, looking, far);
        let mut cut_shape = Shape::default();
        shape(&opened, workpiece_core::Pose::IDENTITY, &mut cut_shape);
        let views: [(&'static str, Point3, Place, Point2, Point2); 4] = [
            (
                "SECTION",
                looking,
                seen,
                Point2::new(size.x + gap, size.z + gap),
                extent,
            ),
            (
                "FRONT",
                -Point3::Y,
                Box::new(move |p| Point2::new(p.x - lo.x, p.z - lo.z)),
                Point2::ZERO,
                Point2::new(size.x, size.z),
            ),
            (
                "TOP",
                Point3::Z,
                Box::new(move |p| Point2::new(p.x - lo.x, p.y - lo.y)),
                Point2::new(0.0, size.z + gap),
                Point2::new(size.x, size.y),
            ),
            (
                "RIGHT",
                Point3::X,
                Box::new(move |p| Point2::new(p.y - lo.y, p.z - lo.z)),
                Point2::new(size.x + gap, 0.0),
                Point2::new(size.y, size.z),
            ),
        ];
        let mut drawn = Vec::new();
        for (name, towards, place, corner, extent) in views {
            let shape_of = if name == "SECTION" {
                &cut_shape
            } else {
                &shape_of
            };
            let mut pieces: Vec<(Point3, Point3)> = Vec::new();
            for run in &shape_of.runs {
                pieces.extend(run.windows(2).map(|w| (w[0], w[1])));
            }
            // The outlines of round faces: where they turn away.
            for (p, n) in &shape_of.triangles {
                let d = n.map(|n| n.dot(towards));
                let mut ends = Vec::new();
                for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                    if (d[i] > 0.0) != (d[j] > 0.0) {
                        ends.push(p[i].lerp(p[j], d[i] / (d[i] - d[j])));
                    }
                }
                if let [a, b] = ends[..] {
                    pieces.push((a, b));
                }
            }
            let mut lines = Vec::new();
            for (a, b) in pieces {
                let (flat_a, flat_b) = (place(a), place(b));
                // (An edge seen end on is a point: not drawn.)
                if flat_a.distance(flat_b) <= reach * 1e-9 {
                    continue;
                }
                let behind = hidden(&shape_of.triangles, (a + b) / 2.0, towards, reach * 1e-6);
                lines.push((corner + flat_a, corner + flat_b, behind));
            }
            // The threads that are only said of the model, as drawings
            // show them (see [`crate::lay::threads`]).
            if !world.threads.is_empty() {
                let cut = (name == "SECTION")
                    .then(|| workpiece_core::world::Cutting::plane(through, looking));
                let behind = |p: Point3| hidden(&shape_of.triangles, p, towards, reach * 2e-3);
                let flat = |p: Point3| corner + place(p);
                let said = crate::lay::threads::see(
                    &world.threads,
                    towards,
                    &flat,
                    &behind,
                    cut.as_ref(),
                    reach,
                );
                lines.retain(|(a, b, _)| !said.leaves_out(*a, *b));
                lines.extend(said.roots);
                lines.extend(said.ends);
                for (at, radius, behind) in said.rounds {
                    let on = |k: usize| {
                        let angle = std::f64::consts::FRAC_PI_2
                            + 0.12
                            + (std::f64::consts::TAU * 0.75 - 0.24) * k as f64 / 36.0;
                        at + from_angle(angle) * radius
                    };
                    lines.extend((0..36).map(|k| (on(k), on(k + 1), behind)));
                }
            }
            // What is round and seen square on: the diameter of a whole
            // circle (in one edge or in several), the radius of an arc.
            let mut rounds: Vec<(Point2, f64, f64)> = Vec::new();
            for (at, axis, radius, around) in &shape_of.circles {
                if axis.dot(towards).abs() < 1.0 - 1e-9 {
                    continue;
                }
                let at = corner + place(*at);
                let same = rounds.iter_mut().find(|r| {
                    r.0.distance(at) <= reach * 1e-7 && (r.1 - radius).abs() <= reach * 1e-7
                });
                match same {
                    Some(found) => found.2 += around,
                    None => rounds.push((at, *radius, *around)),
                }
            }
            // (Two circles one behind the other are one in the view: the
            // rims of a hole.)
            let notes: Vec<(Point2, String)> = rounds
                .into_iter()
                .map(|(at, radius, around)| {
                    let whole = around >= std::f64::consts::TAU * (1.0 - 1e-6);
                    let text = if whole {
                        format!("DIA {}", number(2.0 * radius))
                    } else {
                        format!("R {}", number(radius))
                    };
                    (at, text)
                })
                .collect();
            drawn.push(View {
                name,
                lines,
                corner,
                size: extent,
                notes,
            });
        }
        Ok(Self {
            views: drawn,
            size: Point2::new(
                size.x + gap + size.x.max(size.y),
                size.z + gap + size.y.max(size.z),
            ),
            title: title.to_owned(),
            section: (axis, along.clamp(0.0, 1.0)),
            dimensions: Vec::new(),
        })
    }

    /// The drawing as a DXF file with its numbers in `unit`: each view
    /// on a layer of its own (`FRONT`), what is hidden on another
    /// (`FRONT_HIDDEN`, dashed), and each view's width and height
    /// written beside it (`DIMENSIONS`).
    pub fn dxf(&self, unit: Unit) -> String {
        // (What units a drawing's numbers are: 1 inches, 4 millimetres,
        // 5 centimetres, 6 metres.)
        let units = match unit {
            Unit::Inch => 1,
            Unit::Millimetre => 4,
            Unit::Centimetre => 5,
            Unit::Metre => 6,
        };
        let mut out = format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{units}\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n70\n1\n0\nLTYPE\n2\nHIDDEN\n70\n0\n3\nHidden\n72\n65\n73\n2\n40\n{dash:.6}\n49\n{on:.6}\n49\n-{off:.6}\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n",
            dash = self.size.max_element() * 0.015,
            on = self.size.max_element() * 0.01,
            off = self.size.max_element() * 0.005,
        );
        let high = self.size.max_element() * 0.02;
        for view in &self.views {
            for (a, b, behind) in &view.lines {
                let (layer, kind) = if *behind {
                    (format!("{}_HIDDEN", view.name), "6\nHIDDEN\n")
                } else {
                    (view.name.to_owned(), "")
                };
                let _ = write!(
                    out,
                    "0\nLINE\n8\n{layer}\n{kind}10\n{:.9}\n20\n{:.9}\n30\n0.0\n11\n{:.9}\n21\n{:.9}\n31\n0.0\n",
                    a.x, a.y, b.x, b.y
                );
            }
            let at = view.corner + Point2::new(0.0, -high * 2.0);
            let mut texts = vec![(at, self.says(view, unit))];
            texts.extend(view.notes.iter().cloned());
            for (at, text) in texts {
                let _ = write!(
                    out,
                    "0\nTEXT\n8\nDIMENSIONS\n10\n{:.9}\n20\n{:.9}\n30\n0.0\n40\n{high:.9}\n1\n{text}\n",
                    at.x, at.y
                );
            }
        }
        // The title block, under the sheet's right.
        for (row, text) in self.title_lines(unit, None).iter().enumerate() {
            let at = Point2::new(self.size.x * 0.6, -high * (5.0 + 2.0 * row as f64));
            let _ = write!(
                out,
                "0\nTEXT\n8\nTITLE\n10\n{:.9}\n20\n{:.9}\n30\n0.0\n40\n{high:.9}\n1\n{text}\n",
                at.x, at.y
            );
        }
        out.push_str("0\nENDSEC\n0\nEOF\n");
        out
    }

    /// What the title block says: what is drawn, in what unit, how the
    /// views are laid out, and (on a sheet) at what scale.
    fn title_lines(&self, unit: Unit, scale: Option<f64>) -> Vec<String> {
        let mut lines = vec![
            self.title.clone(),
            format!("Unit: {}   Third-angle projection", unit.label()),
        ];
        // (As plain letters: what writes the sheet knows no others.)
        let plain = |text: &str| {
            text.replace('Ø', "DIA ")
                .replace('±', "+/-")
                .replace('°', " deg")
        };
        for (sketch, said) in &self.dimensions {
            if !said.is_empty() {
                lines.push(plain(&format!("{sketch}: {}", said.join(", "))));
            }
        }
        if let Some(scale) = scale {
            lines.push(if scale >= 1.0 {
                format!("Scale {} : 1", number(scale))
            } else {
                format!("Scale 1 : {}", number(1.0 / scale))
            });
        }
        lines
    }

    /// What is written under a view: its name and how big it is.
    fn says(&self, view: &View, unit: Unit) -> String {
        let (wide, high) = (number(view.size.x), number(view.size.y));
        let name = match view.name {
            "SECTION" => {
                let (axis, along) = self.section;
                let axis = match axis {
                    workpiece_core::math::Axis::X => "x",
                    workpiece_core::math::Axis::Y => "y",
                    workpiece_core::math::Axis::Z => "z",
                };
                format!(
                    "SECTION A-A (square to {axis}, {}% along)",
                    number(along * 100.0)
                )
            }
            other => other.to_owned(),
        };
        format!("{name} {wide} x {high} {}", unit.label())
    }

    /// The drawing as a one-page PDF file, as large as fits an A4 sheet
    /// on its side: what is in view drawn solid, what is hidden thin and
    /// dashed, each view's width and height written under it.
    pub fn pdf(&self, unit: Unit) -> Vec<u8> {
        let (sheet, margin) = (Point2::new(842.0, 595.0), 40.0);
        let room = sheet - Point2::splat(margin * 2.0) - Point2::new(0.0, 24.0);
        let scale = (room.x / self.size.x).min(room.y / self.size.y);
        let place = |p: Point2| Point2::new(margin, margin + 24.0) + p * scale;
        let mut page = String::new();
        for behind in [true, false] {
            page.push_str(if behind {
                "0.3 w [3 2] 0 d 0.45 G\n"
            } else {
                "0.8 w [] 0 d 0 G\n"
            });
            for view in &self.views {
                for (a, b, is_behind) in &view.lines {
                    if *is_behind == behind {
                        let (a, b) = (place(*a), place(*b));
                        let _ = writeln!(page, "{:.3} {:.3} m {:.3} {:.3} l", a.x, a.y, b.x, b.y);
                    }
                }
            }
            page.push_str("S\n");
        }
        for view in &self.views {
            let at = place(view.corner) - Point2::new(0.0, 14.0);
            let _ = writeln!(
                page,
                "BT /F1 9 Tf {:.3} {:.3} Td ({}) Tj ET",
                at.x,
                at.y,
                self.says(view, unit)
            );
        }
        for view in &self.views {
            for (at, text) in &view.notes {
                let at = place(*at) + Point2::new(3.0, 3.0);
                let _ = writeln!(
                    page,
                    "BT /F1 7 Tf {:.3} {:.3} Td ({text}) Tj ET",
                    at.x, at.y
                );
            }
        }
        // The title block: a frame at the sheet's lower right. (A point
        // of the sheet is a 72nd of an inch: what the scale is told by.)
        let true_scale = scale * 25.4 / 72.0 / unit.millimetres();
        let rows = self.title_lines(unit, Some(true_scale)).len();
        let (wide, high) = (300.0, 7.0 + 13.0 * rows as f64);
        let corner = Point2::new(sheet.x - margin - wide, margin * 0.5);
        let _ = writeln!(
            page,
            "0.8 w [] 0 d 0 G {:.3} {:.3} {wide} {high} re S",
            corner.x, corner.y
        );
        for (row, text) in self.title_lines(unit, Some(true_scale)).iter().enumerate() {
            let at = corner + Point2::new(6.0, high - 13.0 * (row as f64 + 1.0));
            let text = text.replace('\\', "").replace(['(', ')'], "");
            let _ = writeln!(
                page,
                "BT /F1 9 Tf {:.3} {:.3} Td ({text}) Tj ET",
                at.x, at.y
            );
        }
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
                sheet.x, sheet.y
            ),
            format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        let mut out = String::from("%PDF-1.4\n");
        let mut starts = Vec::new();
        for (k, object) in objects.iter().enumerate() {
            starts.push(out.len());
            let _ = write!(out, "{} 0 obj\n{object}\nendobj\n", k + 1);
        }
        let table = out.len();
        let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for start in starts {
            let _ = writeln!(out, "{start:010} 00000 n ");
        }
        let _ = write!(
            out,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n",
            objects.len() + 1
        );
        out.into_bytes()
    }
}
