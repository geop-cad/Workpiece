//! Writing STEP files: solids as AP214 boundary representations, every
//! curve and surface a B-spline — what they are here.

use std::collections::BTreeMap;
use std::fmt::Write;

use workpiece_boolean::model::{Face, Walk};
use workpiece_boolean::{Solid, measure};
use workpiece_core::math::Point3;
use workpiece_core::world::topo::{Curve3, NurbsSurface};

/// The instances of a file being written, numbered as they come.
struct Writer {
    text: String,
    next: u32,
}

impl Writer {
    fn add(&mut self, body: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        let _ = writeln!(self.text, "#{id}={body};");
        id
    }
}

/// A number as STEP writes one: always with its point.
fn real(x: f64) -> String {
    let x = if x == 0.0 { 0.0 } else { x };
    let text = format!("{x:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if mantissa.contains('.') => format!("{mantissa}E{exponent}"),
        Some((mantissa, exponent)) => format!("{mantissa}.E{exponent}"),
        None => text,
    }
}

fn reals(xs: impl IntoIterator<Item = f64>) -> String {
    let all: Vec<String> = xs.into_iter().map(real).collect();
    all.join(",")
}

fn refs(ids: impl IntoIterator<Item = u32>) -> String {
    let all: Vec<String> = ids.into_iter().map(|id| format!("#{id}")).collect();
    all.join(",")
}

/// A name as a STEP string: what of it a string there may hold.
fn string(name: &str) -> String {
    let kept: String = name
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control() && *c != '\\')
        .collect();
    format!("'{}'", kept.replace('\'', "''"))
}

/// The distinct knots of a knot vector, and how often each comes.
fn knots(all: &[f64]) -> (String, String) {
    let mut distinct: Vec<(f64, usize)> = Vec::new();
    for &k in all {
        match distinct.last_mut() {
            Some(last) if last.0 == k => last.1 += 1,
            _ => distinct.push((k, 1)),
        }
    }
    let times: Vec<String> = distinct.iter().map(|d| d.1.to_string()).collect();
    (times.join(","), reals(distinct.iter().map(|d| d.0)))
}

impl Writer {
    fn point(&mut self, p: Point3) -> u32 {
        self.add(&format!("CARTESIAN_POINT('',({}))", reals(p.to_array())))
    }

    /// `curve` as what it is — a line, a circle — or as the B-spline
    /// it is here.
    fn curve(&mut self, curve: &Curve3) -> u32 {
        let (a, b) = curve.domain();
        let at: Vec<Point3> = (0..=16)
            .map(|k| curve.point_at(a + (b - a) * f64::from(k) / 16.0))
            .collect();
        let (first, last) = (at[0], at[16]);
        let long: f64 = at.windows(2).map(|w| w[0].distance(w[1])).sum();
        let within = long * 1e-10;
        if let Some(way) = (last - first).try_normalize()
            && at
                .iter()
                .all(|p| (*p - first).reject_from_normalized(way).length() <= within)
        {
            let (from, way) = (self.point(first), self.direction(way));
            let along = self.add(&format!("VECTOR('',#{way},1.)"));
            return self.add(&format!("LINE('',#{from},#{along})"));
        }
        // The circle through three of its places, if all are on it:
        // turning the way the curve runs.
        let (p, q) = (at[5], at[11]);
        let normal = (p - first).cross(q - first);
        let area = normal.length_squared();
        if area > 1e-18 * long.powi(4) {
            let (ab, ac) = (p - first, q - first);
            let centre = first
                + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
                    / (2.0 * area);
            let normal = normal.normalize();
            let radius = centre.distance(first);
            let round = at.iter().all(|x| {
                let d = *x - centre;
                (d.length() - radius).abs() <= within && d.dot(normal).abs() <= within
            });
            if round {
                let across = (first - centre) / radius;
                let (at, axis, across) = (
                    self.point(centre),
                    self.direction(normal),
                    self.direction(across),
                );
                let placed = self.add(&format!("AXIS2_PLACEMENT_3D('',#{at},#{axis},#{across})"));
                return self.add(&format!("CIRCLE('',#{placed},{})", real(radius)));
            }
        }
        let points: Vec<u32> = curve
            .control_points()
            .iter()
            .map(|p| self.point(*p))
            .collect();
        let degree = curve.degree();
        let (times, values) = knots(curve.knots());
        if !curve.is_rational() {
            return self.add(&format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',{degree},({}),.UNSPECIFIED.,.F.,.F.,({times}),({values}),.UNSPECIFIED.)",
                refs(points)
            ));
        }
        self.add(&format!(
            "(BOUNDED_CURVE()B_SPLINE_CURVE({degree},({}),.UNSPECIFIED.,.F.,.F.)B_SPLINE_CURVE_WITH_KNOTS(({times}),({values}),.UNSPECIFIED.)CURVE()GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE(({}))REPRESENTATION_ITEM(''))",
            refs(points),
            reals(curve.weights().iter().copied())
        ))
    }

    /// `surface` as what it is — a plane, a cylinder, a cone, a sphere:
    /// what another system knows best — or, being none of them, as the
    /// B-spline it is here. The second value says whether what is
    /// written faces the other way than `surface`'s own normal.
    fn surface(&mut self, surface: &NurbsSurface) -> (u32, bool) {
        let Some(known) = known(surface) else {
            return (self.spline(surface), false);
        };
        let place = |w: &mut Writer, at: Point3, axis: Point3, across: Point3| {
            let (at, axis, across) = (w.point(at), w.direction(axis), w.direction(across));
            w.add(&format!("AXIS2_PLACEMENT_3D('',#{at},#{axis},#{across})"))
        };
        match known {
            Known::Plane { at, normal, across } => {
                let placed = place(self, at, normal, across);
                (self.add(&format!("PLANE('',#{placed})")), false)
            }
            Known::Round {
                at,
                axis,
                across,
                radius,
                slope,
                inward,
            } => {
                let placed = place(self, at, axis, across);
                let radius = real(radius);
                let id = if slope == 0.0 {
                    self.add(&format!("CYLINDRICAL_SURFACE('',#{placed},{radius})"))
                } else {
                    let angle = real(libm::atan(slope));
                    self.add(&format!("CONICAL_SURFACE('',#{placed},{radius},{angle})"))
                };
                (id, inward)
            }
            Known::Ball {
                at,
                axis,
                across,
                radius,
                inward,
            } => {
                let placed = place(self, at, axis, across);
                let id = self.add(&format!("SPHERICAL_SURFACE('',#{placed},{})", real(radius)));
                (id, inward)
            }
            Known::Ring {
                at,
                axis,
                across,
                major,
                minor,
                inward,
            } => {
                let placed = place(self, at, axis, across);
                let (major, minor) = (real(major), real(minor));
                let id = self.add(&format!("TOROIDAL_SURFACE('',#{placed},{major},{minor})"));
                (id, inward)
            }
        }
    }

    fn direction(&mut self, d: Point3) -> u32 {
        self.add(&format!("DIRECTION('',({}))", reals(d.to_array())))
    }

    fn spline(&mut self, surface: &NurbsSurface) -> u32 {
        let (p, q) = surface.degrees();
        let (nu, nv) = surface.counts();
        let (ku, kv) = surface.knots();
        let mut rows = Vec::new();
        for i in 0..nu {
            let row: Vec<u32> = (0..nv)
                .map(|j| self.point(surface.control_points()[i * nv + j]))
                .collect();
            rows.push(format!("({})", refs(row)));
        }
        let rows = rows.join(",");
        let ((tu, vu), (tv, vv)) = (knots(ku), knots(kv));
        let weights = surface.weights();
        let rational = weights.iter().any(|w| (*w - weights[0]).abs() > 0.0);
        if !rational {
            return self.add(&format!(
                "B_SPLINE_SURFACE_WITH_KNOTS('',{p},{q},({rows}),.UNSPECIFIED.,.F.,.F.,.F.,({tu}),({tv}),({vu}),({vv}),.UNSPECIFIED.)"
            ));
        }
        let weights: Vec<String> = (0..nu)
            .map(|i| format!("({})", reals((0..nv).map(|j| weights[i * nv + j]))))
            .collect();
        self.add(&format!(
            "(BOUNDED_SURFACE()B_SPLINE_SURFACE({p},{q},({rows}),.UNSPECIFIED.,.F.,.F.,.F.)B_SPLINE_SURFACE_WITH_KNOTS(({tu}),({tv}),({vu}),({vv}),.UNSPECIFIED.)GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_SURFACE(({}))REPRESENTATION_ITEM('')SURFACE())",
            weights.join(",")
        ))
    }

    /// The faces `faces` of `solid` as a closed shell; turned inside
    /// out, for the shell around a hollow.
    fn shell(
        &mut self,
        solid: &Solid,
        faces: &[usize],
        hollow: bool,
        vertices: &mut BTreeMap<usize, u32>,
        edges: &mut BTreeMap<usize, u32>,
    ) -> u32 {
        let mut made = Vec::new();
        for &f in faces {
            let face: &Face = &solid.faces[f];
            let (surface, turned) = self.surface(&face.surface);
            // Whether the face's own normal is the surface's: out of
            // the material — or, around a hollow, into it (the shell is
            // then used turned around).
            let same = face.outward != hollow;
            let mut bounds = Vec::new();
            for walk in &face.loops {
                let mut uses: Vec<(usize, bool)> = walk
                    .iter()
                    .filter_map(|step| match step.walk {
                        Walk::Edge { edge, reversed } => Some((edge, reversed)),
                        Walk::Pole { .. } => None,
                    })
                    .collect();
                if uses.is_empty() {
                    continue;
                }
                // (Loops run with the face on their left, seen along
                // the face's own normal.)
                if !same {
                    uses.reverse();
                    for u in &mut uses {
                        u.1 = !u.1;
                    }
                }
                let mut oriented = Vec::new();
                for (e, reversed) in uses {
                    let edge = &solid.edges[e];
                    let id = match edges.get(&e) {
                        Some(id) => *id,
                        None => {
                            let mut ends = [0; 2];
                            for (k, v) in edge.ends.into_iter().enumerate() {
                                ends[k] = match vertices.get(&v) {
                                    Some(id) => *id,
                                    None => {
                                        let at = self.point(solid.vertices[v].point);
                                        let id = self.add(&format!("VERTEX_POINT('',#{at})"));
                                        vertices.insert(v, id);
                                        id
                                    }
                                };
                            }
                            let piece = edge.curve.trimmed(edge.range.0, edge.range.1);
                            let curve = self.curve(piece.as_ref().unwrap_or(&edge.curve));
                            let id = self.add(&format!(
                                "EDGE_CURVE({},#{},#{},#{curve},.T.)",
                                string(edge.name.as_deref().unwrap_or("")),
                                ends[0],
                                ends[1]
                            ));
                            edges.insert(e, id);
                            id
                        }
                    };
                    let sense = if reversed { ".F." } else { ".T." };
                    oriented.push(self.add(&format!("ORIENTED_EDGE('',*,*,#{id},{sense})")));
                }
                let around = self.add(&format!("EDGE_LOOP('',({}))", refs(oriented)));
                bounds.push(self.add(&format!("FACE_BOUND('',#{around},.T.)")));
            }
            let sense = if same != turned { ".T." } else { ".F." };
            made.push(self.add(&format!(
                "ADVANCED_FACE({},({}),#{surface},{sense})",
                string(face.name.as_deref().unwrap_or("")),
                refs(bounds)
            )));
        }
        self.add(&format!("CLOSED_SHELL('',({}))", refs(made)))
    }
}

/// The faces of `solid` in shells: those that hang together by their
/// edges.
fn shells(solid: &Solid) -> Vec<Vec<usize>> {
    let mut of_edge: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (f, face) in solid.faces.iter().enumerate() {
        for step in face.loops.iter().flatten() {
            if let Walk::Edge { edge, .. } = step.walk {
                of_edge.entry(edge).or_default().push(f);
            }
        }
    }
    let mut shell_of = vec![usize::MAX; solid.faces.len()];
    let mut all = Vec::new();
    for start in 0..solid.faces.len() {
        if shell_of[start] != usize::MAX {
            continue;
        }
        let mut found = vec![start];
        shell_of[start] = all.len();
        let mut k = 0;
        while k < found.len() {
            let f = found[k];
            k += 1;
            for step in solid.faces[f].loops.iter().flatten() {
                let Walk::Edge { edge, .. } = step.walk else {
                    continue;
                };
                for &g in &of_edge[&edge] {
                    if shell_of[g] == usize::MAX {
                        shell_of[g] = all.len();
                        found.push(g);
                    }
                }
            }
        }
        found.sort_unstable();
        all.push(found);
    }
    all
}

/// A STEP file of `bodies` (each with its name), in millimetres: every
/// solid a `MANIFOLD_SOLID_BREP` — or, with hollows, a
/// `BREP_WITH_VOIDS` — of faces on B-spline surfaces.
pub fn export(
    name: &str,
    bodies: &[(String, Solid)],
    unit: workpiece_core::Unit,
) -> anyhow::Result<String> {
    let mut w = Writer {
        text: String::new(),
        next: 1,
    };
    let context =
        w.add("APPLICATION_CONTEXT('core data for automotive mechanical design processes')");
    w.add(&format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{context})"
    ));
    let product_context = w.add(&format!("PRODUCT_CONTEXT('',#{context},'mechanical')"));
    let product = w.add(&format!(
        "PRODUCT({0},{0},'',(#{product_context}))",
        string(name)
    ));
    let formation = w.add(&format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
    let design = w.add(&format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{context},'design')"
    ));
    let definition = w.add(&format!(
        "PRODUCT_DEFINITION('design','',#{formation},#{design})"
    ));
    let shape = w.add(&format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
    let length = match unit {
        workpiece_core::Unit::Millimetre => {
            w.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))")
        }
        workpiece_core::Unit::Centimetre => {
            w.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.))")
        }
        workpiece_core::Unit::Metre => w.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.))"),
        workpiece_core::Unit::Inch => {
            // An inch is told as so many millimetres.
            let base = w.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
            let size = w.add(&format!(
                "LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{base})"
            ));
            let dimensions = w.add("DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.)");
            w.add(&format!(
                "(CONVERSION_BASED_UNIT('INCH',#{size})LENGTH_UNIT()NAMED_UNIT(#{dimensions}))"
            ))
        }
    };
    let angle = w.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))");
    let solid_angle = w.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())");
    // How near is the same in the file: as near as the bodies
    // themselves hold their vertices, edges and faces together (a
    // reader that takes it wider finds corners of a sliver in one
    // place).
    let near = bodies
        .iter()
        .map(|(_, solid)| solid.tolerance())
        .fold(1e-7, f64::max);
    let closeness = w.add(&format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}),#{length},'distance_accuracy_value','')",
        real(near)
    ));
    let geometry = w.add(&format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{closeness}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{angle},#{solid_angle}))REPRESENTATION_CONTEXT('',''))"
    ));
    let origin = w.point(Point3::ZERO);
    let up = w.add("DIRECTION('',(0.,0.,1.))");
    let along = w.add("DIRECTION('',(1.,0.,0.))");
    let placement = w.add(&format!("AXIS2_PLACEMENT_3D('',#{origin},#{up},#{along})"));
    let mut items = vec![placement];
    for (called, body) in bodies {
        // Every lump a solid of its own: its outside, and the shells
        // around its hollows.
        let lumps = body.lumps()?;
        let many = lumps.len() > 1;
        for (k, lump) in lumps.iter().enumerate() {
            let called = if many {
                format!("{called}.{}", k + 1)
            } else {
                called.clone()
            };
            let (mut vertices, mut edges) = (BTreeMap::new(), BTreeMap::new());
            let mut outside = None;
            let mut hollows = Vec::new();
            for faces in shells(lump) {
                let alone = Solid {
                    faces: faces.iter().map(|&f| lump.faces[f].clone()).collect(),
                    ..lump.clone()
                };
                let hollow = measure(&alone).volume < 0.0;
                let shell = w.shell(lump, &faces, hollow, &mut vertices, &mut edges);
                if hollow {
                    hollows.push(w.add(&format!("ORIENTED_CLOSED_SHELL('',*,#{shell},.F.)")));
                } else {
                    anyhow::ensure!(outside.is_none(), "{called} has two outsides");
                    outside = Some(shell);
                }
            }
            let outside = outside.ok_or_else(|| anyhow::anyhow!("{called} has no outside"))?;
            items.push(if hollows.is_empty() {
                w.add(&format!(
                    "MANIFOLD_SOLID_BREP({},#{outside})",
                    string(&called)
                ))
            } else {
                w.add(&format!(
                    "BREP_WITH_VOIDS({},#{outside},({}))",
                    string(&called),
                    refs(hollows)
                ))
            });
        }
    }
    let representation = w.add(&format!(
        "ADVANCED_BREP_SHAPE_REPRESENTATION('',({}),#{geometry})",
        refs(items)
    ));
    w.add(&format!(
        "SHAPE_DEFINITION_REPRESENTATION(#{shape},#{representation})"
    ));
    Ok(format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Workpiece model'),'2;1');\nFILE_NAME({},'',(''),(''),'workpiece','workpiece','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n{}ENDSEC;\nEND-ISO-10303-21;\n",
        string(name),
        w.text
    ))
}

/// A surface another system has a name for.
enum Known {
    /// A plane through `at` across `normal`.
    Plane {
        at: Point3,
        normal: Point3,
        across: Point3,
    },
    /// A cylinder or a cone about the axis through `at`: of `radius`
    /// there, growing by `slope` for every unit along the axis.
    Round {
        at: Point3,
        axis: Point3,
        across: Point3,
        radius: f64,
        slope: f64,
        inward: bool,
    },
    /// A sphere about `at`.
    Ball {
        at: Point3,
        axis: Point3,
        across: Point3,
        radius: f64,
        inward: bool,
    },
    /// A torus about the axis through `at`: a circle of radius `minor`
    /// whose centre is `major` from the axis, turned around it.
    Ring {
        at: Point3,
        axis: Point3,
        across: Point3,
        major: f64,
        minor: f64,
        inward: bool,
    },
}

/// The torus the places `seen` of `surface` (each with the surface's
/// normal there) all lie on, if there is one: its axis is that of the
/// circle the surface runs along one of its parameters.
fn ring(surface: &NurbsSurface, seen: &[(Point3, Point3)], within: f64) -> Option<Known> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    (0..2).find_map(|along| {
        // The circle through three places along that parameter.
        let at = |f: f64| {
            if along == 0 {
                surface.point_at(u0 + (u1 - u0) * f, vm)
            } else {
                surface.point_at(um, v0 + (v1 - v0) * f)
            }
        };
        let (a, b, c) = (at(0.1), at(0.45), at(0.8));
        let (ab, ac) = (b - a, c - a);
        let normal = ab.cross(ac);
        let area = normal.length_squared();
        if area <= 1e-12 * ab.length_squared() * ac.length_squared() {
            return None;
        }
        let centre = a
            + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
                / (2.0 * area);
        let axis = normal.normalize();
        // Each place, seen in the half plane through the axis it is
        // in: how far out, how high — and its normal there, which is
        // all in that half plane.
        let mut flat = Vec::with_capacity(seen.len());
        for (p, n) in seen {
            let d = *p - centre;
            let high = d.dot(axis);
            let out = d - axis * high;
            let far = out.length();
            if far <= within {
                return None;
            }
            let out = out / far;
            let (n_out, n_high) = (n.dot(out), n.dot(axis));
            if (*n - out * n_out - axis * n_high).length() > 1e-9 {
                return None;
            }
            flat.push(((far, high), (n_out, n_high)));
        }
        // Where the normals of two of them cross: the middle of the
        // tube.
        let (q0, m0) = flat[0];
        let other = flat.iter().max_by(|a, b| {
            let turn = |s: &((f64, f64), (f64, f64))| (s.1.0 * m0.1 - s.1.1 * m0.0).abs();
            turn(a).total_cmp(&turn(b))
        })?;
        let (q1, m1) = *other;
        let det = m0.0 * m1.1 - m0.1 * m1.0;
        if det.abs() <= 1e-6 {
            return None;
        }
        let t = ((q1.0 - q0.0) * m1.1 - (q1.1 - q0.1) * m1.0) / det;
        let (major, height) = (q0.0 + m0.0 * t, q0.1 + m0.1 * t);
        // (Along the normal from the middle of the tube: out of it, or
        // — less than nothing — into it.)
        let minor = -t;
        let fits = flat.iter().all(|((far, high), (n_out, n_high))| {
            let (x, y) = (far - major - n_out * minor, high - height - n_high * minor);
            libm::hypot(x, y) <= within
        });
        // (A ring with a hole in its middle: not one that runs through
        // its own axis.)
        if !fits || minor.abs() <= within || major <= minor.abs() * (1.0 + 1e-6) + within {
            return None;
        }
        let at = centre + axis * height;
        let seam = surface.point_at(u0, v0) - at;
        let across = seam
            .reject_from_normalized(axis)
            .try_normalize()
            .unwrap_or_else(|| axis.any_orthonormal_vector());
        Some(Known::Ring {
            at,
            axis,
            across,
            major,
            minor: minor.abs(),
            inward: minor < 0.0,
        })
    })
}

/// What `surface` is, if it is a plane, a cylinder, a cone or a sphere:
/// told from places all over it and its normals there.
fn known(surface: &NurbsSurface) -> Option<Known> {
    let ((u0, u1), (v0, v1)) = surface.domain();
    let mut seen = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let (u, v) = (
                u0 + (u1 - u0) * (0.08 + 0.21 * f64::from(i)),
                v0 + (v1 - v0) * (0.08 + 0.21 * f64::from(j)),
            );
            let (p, du, dv) = surface.derivatives_at(u, v);
            seen.push((p, du.cross(dv).try_normalize()?));
        }
    }
    let size = seen
        .iter()
        .map(|s| s.0.distance(seen[0].0))
        .fold(0.0, f64::max);
    let within = size * 1e-9;
    let (p0, n0) = seen[0];
    // A plane: one normal everywhere, and every place in it.
    if seen
        .iter()
        .all(|(p, n)| n.distance(n0) <= 1e-9 && (*p - p0).dot(n0).abs() <= within)
    {
        return Some(Known::Plane {
            at: p0,
            normal: n0,
            across: n0.any_orthonormal_vector(),
        });
    }
    // A sphere: every normal through one point, the same way from it.
    let far = seen
        .iter()
        .max_by(|a, b| a.1.distance(n0).total_cmp(&b.1.distance(n0)))?;
    let radius = (p0 - far.0).dot(n0 - far.1) / (n0 - far.1).length_squared();
    let centre = p0 - n0 * radius;
    if seen
        .iter()
        .all(|(p, n)| (*p - *n * radius).distance(centre) <= within)
    {
        // Its axis through where the surface comes to a point, if it
        // does — at the ends of its second parameter or of its first,
        // at both ends or at one (a cap, whose other end is a circle);
        // its seam where the surface's own is. A face around such a
        // point is cut open from there, here as in the file.
        let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let pole = |first: bool, at: f64| {
            let on = |k: i32| {
                let f = f64::from(k) / 8.0;
                if first {
                    surface.point_at(at, v0 + (v1 - v0) * f)
                } else {
                    surface.point_at(u0 + (u1 - u0) * f, at)
                }
            };
            (1..=8).all(|k| on(k).distance(on(0)) <= within)
        };
        let through = |low: Point3, high: Point3, poles: (bool, bool)| match poles {
            (false, true) => high - centre,
            (true, false) => centre - low,
            _ => high - low,
        };
        let (axis, seam) = match (
            (pole(false, v0), pole(false, v1)),
            (pole(true, u0), pole(true, u1)),
        ) {
            ((false, false), around @ ((true, _) | (_, true))) => (
                through(surface.point_at(u0, vm), surface.point_at(u1, vm), around),
                surface.point_at(um, v0),
            ),
            (along, _) => (
                through(surface.point_at(u0, v0), surface.point_at(u0, v1), along),
                surface.point_at(u0, vm),
            ),
        };
        let axis = axis.try_normalize().unwrap_or(Point3::Z);
        let seam = seam - centre;
        let across = seam
            .reject_from_normalized(axis)
            .try_normalize()
            .unwrap_or_else(|| axis.any_orthonormal_vector());
        return Some(Known::Ball {
            at: centre,
            axis,
            across,
            radius: radius.abs(),
            inward: radius < 0.0,
        });
    }
    // A torus: every normal through one circle about an axis.
    if let Some(ring) = ring(surface, &seen, within) {
        return Some(ring);
    }
    // A cylinder or a cone: about an axis every normal leans to alike.
    let mut axis = Point3::ZERO;
    for a in &seen {
        for b in &seen {
            let across = (a.1 - n0).cross(b.1 - n0);
            if across.length() > axis.length() {
                axis = across;
            }
        }
    }
    let axis = axis.try_normalize()?;
    let lean = n0.dot(axis);
    if seen.iter().any(|(_, n)| (n.dot(axis) - lean).abs() > 1e-9) || lean.abs() > 0.999 {
        return None;
    }
    // Where the axis is: the normals of two places as high along it
    // meet on it — seen along it, where their lines cross.
    let (e1, e2) = axis.any_orthonormal_pair();
    let flat = |p: Point3| (p.dot(e1), p.dot(e2));
    let (q0, m0) = (flat(p0), flat(n0));
    let other = seen.iter().max_by(|a, b| {
        let turn = |s: &(Point3, Point3)| (flat(s.1).0 * m0.1 - flat(s.1).1 * m0.0).abs();
        turn(a).total_cmp(&turn(b))
    })?;
    let (q1, m1) = (flat(other.0), flat(other.1));
    let det = m0.0 * m1.1 - m0.1 * m1.0;
    if det.abs() <= 1e-6 {
        return None;
    }
    let t = ((q1.0 - q0.0) * m1.1 - (q1.1 - q0.1) * m1.0) / det;
    let on_axis = e1 * (q0.0 + m0.0 * t) + e2 * (q0.1 + m0.1 * t);
    let out = |p: Point3| (p - on_axis).reject_from_normalized(axis);
    let high = |p: Point3| (p - on_axis).dot(axis);
    // Once around its axis at most: what winds about it more often (a
    // thread's crest, which lies on a cylinder turn after turn) is on
    // the cylinder but is not it — a face of it has no one place in
    // the cylinder's own turn.
    let turned = |along_u: bool| -> f64 {
        let steps = 8 * if along_u {
            surface.knots().0.len()
        } else {
            surface.knots().1.len()
        };
        let at = |k: usize| {
            let f = k as f64 / steps as f64;
            let p = if along_u {
                surface.point_at(u0 + (u1 - u0) * f, (v0 + v1) / 2.0)
            } else {
                surface.point_at((u0 + u1) / 2.0, v0 + (v1 - v0) * f)
            };
            let d = out(p);
            libm::atan2(d.dot(e2), d.dot(e1))
        };
        let mut sum = 0.0;
        let mut before = at(0);
        for k in 1..=steps {
            let here = at(k);
            let mut step = here - before;
            step -= (step / std::f64::consts::TAU).round() * std::f64::consts::TAU;
            sum += step;
            before = here;
        }
        sum.abs()
    };
    if turned(true).max(turned(false)) > std::f64::consts::TAU * (1.0 + 1e-9) {
        return None;
    }
    // Wider by as much for every unit along the axis, everywhere.
    let (r0, h0) = (out(p0).length(), high(p0));
    let steep = seen
        .iter()
        .map(|s| (out(s.0).length() - r0, high(s.0) - h0))
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
    // (No wider anywhere than the surface is told apart by: a cylinder,
    // not a cone that opens by a billionth of nothing from a tip far
    // beyond everything.)
    let widest = seen
        .iter()
        .map(|s| (out(s.0).length() - r0).abs())
        .fold(0.0, f64::max);
    let slope = if steep.1.abs() > within && widest > within {
        steep.0 / steep.1
    } else {
        0.0
    };
    let fits = seen.iter().all(|(p, n)| {
        let wide = r0 + slope * (high(*p) - h0);
        let radial = out(*p);
        (radial.length() - wide).abs() <= within
            && radial
                .try_normalize()
                .is_some_and(|r| r.cross(n.reject_from_normalized(axis)).length() <= 1e-7)
    });
    if !fits || r0 <= within {
        return None;
    }
    // (A cone opens along its axis as STEP has it: the axis turned to
    // where it widens.)
    let (axis, slope, h0) = if slope < 0.0 {
        (-axis, -slope, -h0)
    } else {
        (axis, slope, h0)
    };
    let seam = surface.point_at(u0, (v0 + v1) / 2.0) - on_axis;
    let across = seam
        .reject_from_normalized(axis)
        .try_normalize()
        .unwrap_or_else(|| axis.any_orthonormal_vector());
    Some(Known::Round {
        at: on_axis + axis * h0,
        axis,
        across,
        radius: r0,
        slope,
        inward: out(p0).dot(n0) < 0.0,
    })
}
