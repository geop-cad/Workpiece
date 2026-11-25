//! Surfaces told for what they are: a plane, a cylinder. What two such
//! faces can do to each other is known without looking: two planes
//! never cross in a closed curve, a plane along a cylinder's axis
//! crosses it in straight lines, and none of them touch in a single
//! point where branches of a crossing meet.

use shapecraft_core::math::Point3;

use crate::bezier::Piece;

/// What a face's surface is, over the box of its parameters in use.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Known {
    /// A plane, across `normal`.
    Plane {
        /// The way it faces (either way).
        normal: Point3,
    },
    /// A circular cylinder (all or part of the way around).
    Cylinder {
        /// A place on its axis.
        centre: Point3,
        /// The way its axis runs.
        axis: Point3,
        /// Its radius.
        radius: f64,
        /// How long the part in use is along the axis.
        length: f64,
    },
    /// A circular cone (all or part of the way around, between two of
    /// its circles).
    Cone {
        /// The way its axis runs.
        axis: Point3,
        /// The sine of half the angle it opens by.
        opening: f64,
    },
    /// A sphere (all or part of it).
    Sphere {
        /// Its middle.
        centre: Point3,
        /// Its radius.
        radius: f64,
    },
}

/// What is known of the closed curves two faces may cross in.
pub(crate) enum Closed {
    /// There are none inside both.
    Never,
    /// One at most, and it goes through these three places (a closed
    /// curve is drawn from vertex to vertex: one place is not enough).
    Through([Point3; 3]),
    /// Nothing: it is looked for.
    Unknown,
}

/// How many ways there are to take `k` of `n`.
fn choose(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64)
}

/// The Bernstein coefficients of the product of the polynomials with
/// Bernstein coefficients `a` and `b`.
pub(crate) fn product(a: &[f64], b: &[f64]) -> Vec<f64> {
    let (p, q) = (a.len() - 1, b.len() - 1);
    let mut out = vec![0.0; p + q + 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            out[i + j] += choose(p, i) * choose(q, j) / choose(p + q, i + j) * x * y;
        }
    }
    out
}

/// What the surface with the Bézier patches `patches` is, if it is
/// told: to within `1e-10` of its size, by its control points alone
/// (what holds of them holds all over the patches).
pub(crate) fn known(patches: &[Piece]) -> Option<Known> {
    let first = patches.first()?;
    if patches.iter().any(|p| p.dims() != 2) {
        return None;
    }
    let size = patches
        .iter()
        .fold(crate::bezier::Bounds::EMPTY, |b, p| b.union(p.bounds()))
        .size();
    if crate::loops::flat(patches, size * 1e-12) {
        let normal = first.extent(0).cross(first.extent(1)).try_normalize()?;
        return Some(Known::Plane { normal });
    }
    cylinder(patches, size)
        .or_else(|| cone(patches, size))
        .or_else(|| sphere(patches))
}

/// The point a weighted control point stands for.
fn place(h: &[f64; 4]) -> Point3 {
    Point3::new(h[0], h[1], h[2]) / h[3]
}

/// The middle of the circle through three points.
fn circle(a: Point3, b: Point3, c: Point3) -> Option<Point3> {
    let (ab, ac) = (b - a, c - a);
    let normal = ab.cross(ac);
    let area = normal.length_squared();
    if area <= 1e-12 * ab.length_squared() * ac.length_squared() {
        return None;
    }
    Some(
        a + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
            / (2.0 * area),
    )
}

/// Whether the curve with the weighted control points `cp` is on the
/// circle about `centre` across `axis` with `radius`: all of it in the
/// circle's plane, and as far from the middle as the circle is. (The
/// square of how far a place of it is from the middle, less the radius
/// squared, times the weight squared, is a polynomial — nothing, where
/// all its coefficients are.)
fn on_circle(cp: &[[f64; 4]], centre: Point3, axis: Point3, radius: f64, near: f64) -> bool {
    let w: Vec<f64> = cp.iter().map(|h| h[3]).collect();
    if w.iter().any(|x| *x <= 0.0) {
        return false;
    }
    if cp
        .iter()
        .any(|h| (place(h) - centre).dot(axis).abs() > near)
    {
        return false;
    }
    let heavy = w.iter().fold(0.0_f64, |m, x| m.max(*x));
    let mut sum = product(&w, &w);
    for x in &mut sum {
        *x *= -radius * radius;
    }
    for c in 0..3 {
        let q: Vec<f64> = cp
            .iter()
            .map(|h| {
                let off = (place(h) - centre).reject_from_normalized(axis) * h[3];
                off.to_array()[c]
            })
            .collect();
        for (x, y) in sum.iter_mut().zip(product(&q, &q)) {
            *x += y;
        }
    }
    let bound = (1e-10 * radius * radius + near * near) * heavy * heavy;
    sum.iter().all(|x| x.abs() <= bound)
}

/// The patches as a cone: straight one way, between two circles about
/// one axis that turn together.
fn cone(patches: &[Piece], size: f64) -> Option<Known> {
    let first = patches.first()?;
    let straight = usize::from(first.n[1] == 2);
    if first.n[straight] != 2 {
        return None;
    }
    let near = size * 1e-11;
    let row = |p: &Piece, end: usize| -> Vec<[f64; 4]> {
        (0..p.n[1 - straight])
            .map(|k| {
                let (i, j) = if straight == 0 { (end, k) } else { (k, end) };
                p.cp[i * p.n[1] + j]
            })
            .collect()
    };
    let on_row = |cp: &[[f64; 4]], t: f64| {
        let x: Vec<f64> = (0..4)
            .map(|c| crate::bezier::bernstein(&cp.iter().map(|h| h[c]).collect::<Vec<_>>(), t))
            .collect();
        Point3::new(x[0], x[1], x[2]) / x[3]
    };
    // The wider of the first patch's two rows says where the axis is.
    let rows = [row(first, 0), row(first, 1)];
    if rows.iter().flatten().any(|h| h[3] <= 0.0) {
        return None;
    }
    let wide = |cp: &[[f64; 4]]| place(&cp[0]).distance(place(&cp[cp.len() - 1]));
    let of = usize::from(wide(&rows[1]) > wide(&rows[0]));
    let (a, b, c) = (
        on_row(&rows[of], 0.0),
        on_row(&rows[of], 0.5),
        on_row(&rows[of], 1.0),
    );
    let middle = circle(a, b, c)?;
    let axis = (b - a).cross(c - a).try_normalize()?;
    let mut opening: Option<f64> = None;
    for p in patches {
        if p.n[straight] != 2 {
            return None;
        }
        let rows = [row(p, 0), row(p, 1)];
        if rows.iter().flatten().any(|h| h[3] <= 0.0) {
            return None;
        }
        // Each row a circle about the axis.
        let mut circles = [(Point3::ZERO, 0.0); 2];
        for (k, cp) in rows.iter().enumerate() {
            let from = place(&cp[0]);
            let centre = middle + axis * (from - middle).dot(axis);
            let radius = from.distance(centre);
            if !on_circle(cp, centre, axis, radius, near) {
                return None;
            }
            circles[k] = (centre, radius);
        }
        // The two turn together: each place of one straight out from
        // the axis as its partner on the other is.
        let ((c0, r0), (c1, r1)) = (circles[0], circles[1]);
        for (h0, h1) in rows[0].iter().zip(&rows[1]) {
            if (h0[3] - h1[3]).abs() > 1e-12 * h0[3] {
                return None;
            }
            let (out0, out1) = (place(h0) - c0, place(h1) - c1);
            if (out0 * r1 - out1 * r0).length() > near * (r0 + r1) {
                return None;
            }
        }
        let along = c0.distance(c1);
        let slant = along.hypot(r1 - r0);
        if slant <= near {
            return None;
        }
        // (One cone: the same opening all over, the same way up.)
        let sine = (r1 - r0) / slant * (c1 - c0).dot(axis).signum();
        match opening {
            Some(known) if (known - sine).abs() > 1e-10 => return None,
            _ => opening = Some(sine),
        }
    }
    let opening = opening?.abs();
    (opening > 1e-9 && opening < 1.0 - 1e-9).then_some(Known::Cone { axis, opening })
}

/// The patches as a sphere: every place as far from one middle.
fn sphere(patches: &[Piece]) -> Option<Known> {
    let first = patches.first()?;
    // The middle: as far from four places of the first patch.
    let at = [[0.2, 0.3], [0.7, 0.2], [0.4, 0.8], [0.9, 0.9]].map(|x: [f64; 2]| {
        let [(u0, u1), (v0, v1)] = first.range;
        first.eval([u0 + (u1 - u0) * x[0], v0 + (v1 - v0) * x[1]])
    });
    let rows = [at[1] - at[0], at[2] - at[0], at[3] - at[0]];
    let right = [1, 2, 3].map(|k| (at[k].length_squared() - at[0].length_squared()) / 2.0);
    let det = rows[0].dot(rows[1].cross(rows[2]));
    let scale = rows[0].length() * rows[1].length() * rows[2].length();
    if det.abs() <= 1e-9 * scale || scale <= 0.0 {
        return None;
    }
    let centre = (rows[1].cross(rows[2]) * right[0]
        + rows[2].cross(rows[0]) * right[1]
        + rows[0].cross(rows[1]) * right[2])
        / det;
    let radius = centre.distance(at[0]);
    if radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || !radius.is_finite() {
        return None;
    }
    // Every patch on it: the square of how far a place is from the
    // middle, less the radius squared, times the weight squared — a
    // polynomial of the two parameters — is nothing.
    for p in patches {
        let (nu, nv) = (p.n[0], p.n[1]);
        if p.cp.iter().any(|h| h[3] <= 0.0) {
            return None;
        }
        let heavy = p.cp.iter().fold(0.0_f64, |m, h| m.max(h[3]));
        let squared = |f: &dyn Fn(&[f64; 4]) -> f64| -> Vec<f64> {
            // (Row by row along v, then the rows along u.)
            let (mu, mv) = (2 * nu - 1, 2 * nv - 1);
            let mut out = vec![0.0; mu * mv];
            for i in 0..nu {
                for k in 0..nu {
                    let a: Vec<f64> = (0..nv).map(|j| f(&p.cp[i * nv + j])).collect();
                    let b: Vec<f64> = (0..nv).map(|j| f(&p.cp[k * nv + j])).collect();
                    let along = product(&a, &b);
                    let share = choose(nu - 1, i) * choose(nu - 1, k) / choose(2 * nu - 2, i + k);
                    for (j, x) in along.iter().enumerate() {
                        out[(i + k) * mv + j] += share * x;
                    }
                }
            }
            out
        };
        let mut sum = squared(&|h| h[3]);
        for x in &mut sum {
            *x *= -radius * radius;
        }
        for c in 0..3 {
            let part = squared(&|h| h[c] - centre.to_array()[c] * h[3]);
            for (x, y) in sum.iter_mut().zip(part) {
                *x += y;
            }
        }
        let bound = 1e-10 * radius * radius * heavy * heavy;
        if sum.iter().any(|x| x.abs() > bound) {
            return None;
        }
    }
    Some(Known::Sphere { centre, radius })
}

/// The patches as a cylinder: straight one way, a circle the other.
fn cylinder(patches: &[Piece], size: f64) -> Option<Known> {
    let first = patches.first()?;
    // A cylinder: straight one way — every row of control points the
    // same step from the one before, with the same weights — and a
    // circle the other.
    let straight = usize::from(first.n[1] == 2);
    if first.n[straight] != 2 {
        return None;
    }
    let at = |p: &Piece, k: usize, end: usize| {
        let (i, j) = if straight == 0 { (end, k) } else { (k, end) };
        p.cp[i * p.n[1] + j]
    };
    let step = {
        let (a, b) = (at(first, 0, 0), at(first, 0, 1));
        if a[3] <= 0.0 || b[3] <= 0.0 {
            return None;
        }
        Point3::new(b[0], b[1], b[2]) / b[3] - Point3::new(a[0], a[1], a[2]) / a[3]
    };
    let axis = step.try_normalize()?;
    let near = size * 1e-11;
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in patches {
        if p.n[straight] != 2 {
            return None;
        }
        for k in 0..p.n[1 - straight] {
            let (a, b) = (at(p, k, 0), at(p, k, 1));
            if a[3] <= 0.0 || (a[3] - b[3]).abs() > 1e-12 * a[3] {
                return None;
            }
            let (pa, pb) = (
                Point3::new(a[0], a[1], a[2]) / a[3],
                Point3::new(b[0], b[1], b[2]) / b[3],
            );
            // (Parallel to the first, if not as long: patches cut from
            // one cylinder at different heights.)
            if (pb - pa).reject_from_normalized(axis).length() > near {
                return None;
            }
            for q in [pa, pb] {
                (low, high) = (low.min(q.dot(axis)), high.max(q.dot(axis)));
            }
        }
    }
    // The circle through three places of the first row, seen along the
    // axis.
    let across = |q: Point3| q.reject_from_normalized(axis);
    let row =
        |p: &Piece| -> Vec<[f64; 4]> { (0..p.n[1 - straight]).map(|k| at(p, k, 0)).collect() };
    let on_row = |cp: &[[f64; 4]], t: f64| {
        let x: Vec<f64> = (0..4)
            .map(|c| crate::bezier::bernstein(&cp.iter().map(|h| h[c]).collect::<Vec<_>>(), t))
            .collect();
        across(Point3::new(x[0], x[1], x[2]) / x[3])
    };
    let first_row = row(first);
    let (a, b, c) = (
        on_row(&first_row, 0.0),
        on_row(&first_row, 0.5),
        on_row(&first_row, 1.0),
    );
    let (ab, ac) = (b - a, c - a);
    let normal = ab.cross(ac);
    let area = normal.length_squared();
    if area <= 1e-12 * ab.length_squared() * ac.length_squared() {
        return None;
    }
    let centre = a
        + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
            / (2.0 * area);
    let radius = centre.distance(a);
    if radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || !radius.is_finite() {
        return None;
    }
    // Every row is on it: the square of how far a place of the row is
    // from the axis, less the radius squared, times the weight squared,
    // is a polynomial — nothing, where all its coefficients are.
    for p in patches {
        let cp = row(p);
        let w: Vec<f64> = cp.iter().map(|h| h[3]).collect();
        let heavy = w.iter().fold(0.0_f64, |m, x| m.max(*x));
        let mut sum = product(&w, &w);
        for x in &mut sum {
            *x *= -radius * radius;
        }
        for c in 0..3 {
            // (The place across the axis from the centre, weighted.)
            let q: Vec<f64> = cp
                .iter()
                .map(|h| {
                    let whole = Point3::new(h[0], h[1], h[2]);
                    let off = across(whole) - centre * h[3];
                    off.to_array()[c]
                })
                .collect();
            for (x, y) in sum.iter_mut().zip(product(&q, &q)) {
                *x += y;
            }
        }
        let bound = 1e-10 * radius * radius * heavy * heavy;
        if sum.iter().any(|x| x.abs() > bound) {
            return None;
        }
    }
    Some(Known::Cylinder {
        centre,
        axis,
        radius,
        length: high - low,
    })
}

/// Three turns about an axis, a third of the way around apart (and
/// off the turns surfaces are usually begun at).
fn thirds() -> [f64; 3] {
    [0.0, 1.0, 2.0].map(|k| 0.7 + k * std::f64::consts::TAU / 3.0)
}

/// Three places on the circle about `centre` across `axis`.
fn around(centre: Point3, axis: Point3, radius: f64) -> [Point3; 3] {
    let (x, y) = axis.any_orthonormal_pair();
    thirds().map(|turn| centre + (x * turn.cos() + y * turn.sin()) * radius)
}

/// What is known of the closed curves the two may cross in, inside
/// both (as far as each is in use): none where what they cross in
/// reaches an edge whatever the faces are — straight lines, an ellipse
/// longer than the cylinder, a curve that goes on for ever; one, with a
/// place on it, between a sphere and a plane or another sphere that go
/// well through each other. `tol` is how near counts as touching.
pub(crate) fn closed(a: &Known, b: &Known, at: (Point3, Point3), tol: f64) -> Closed {
    use Known::{Cone, Cylinder, Plane, Sphere};
    match (a, b) {
        (Plane { .. }, Plane { .. }) => Closed::Never,
        (
            Plane { normal },
            Cylinder {
                centre,
                axis,
                radius,
                length,
            },
        )
        | (
            Cylinder {
                centre,
                axis,
                radius,
                length,
            },
            Plane { normal },
        ) => {
            // The ellipse they cross in is longer along the axis than
            // the cylinder is (straight lines, with the plane along
            // the axis).
            let lean = normal.dot(*axis).abs();
            let along = 2.0 * radius * (1.0 - lean * lean).max(0.0).sqrt();
            if along > lean * length * (1.0 + 1e-6) {
                return Closed::Never;
            }
            if lean < 0.05 {
                return Closed::Unknown;
            }
            // Where three of the cylinder's straight lines go through
            // the plane.
            let on = if matches!(a, Plane { .. }) {
                at.0
            } else {
                at.1
            };
            let (x, y) = axis.any_orthonormal_pair();
            Closed::Through(thirds().map(|turn| {
                let from = *centre + (x * turn.cos() + y * turn.sin()) * *radius;
                from + *axis * ((on - from).dot(*normal) / normal.dot(*axis))
            }))
        }
        (Plane { normal }, Cone { axis, opening }) | (Cone { axis, opening }, Plane { normal }) => {
            // A plane no steeper to the axis than the cone's side: a
            // parabola, a hyperbola or straight lines.
            if normal.dot(*axis).abs() < opening * (1.0 - 1e-6) {
                Closed::Never
            } else {
                Closed::Unknown
            }
        }
        (Plane { normal }, Sphere { centre, radius })
        | (Sphere { centre, radius }, Plane { normal }) => {
            // (A place of the plane: where the plane's own face is.)
            let on = if matches!(a, Plane { .. }) {
                at.0
            } else {
                at.1
            };
            let off = (*centre - on).dot(*normal);
            if off.abs() > radius + tol * 2.0 {
                return Closed::Never;
            }
            let wide = radius * radius - off * off;
            if off.abs() > radius - tol * 4.0 || wide <= 0.0 || wide.sqrt() <= tol * 64.0 {
                return Closed::Unknown;
            }
            Closed::Through(around(*centre - *normal * off, *normal, wide.sqrt()))
        }
        (Cylinder { axis: da, .. }, Cylinder { axis: db, .. }) => {
            if da.cross(*db).length() <= 1e-12 {
                Closed::Never
            } else {
                Closed::Unknown
            }
        }
        (
            Sphere { centre, radius },
            Sphere {
                centre: other,
                radius: far,
            },
        ) => {
            let apart = centre.distance(*other);
            if apart > radius + far + tol * 2.0 || apart < (radius - far).abs() - tol * 2.0 {
                return Closed::Never;
            }
            let Some(way) = (*other - *centre).try_normalize() else {
                return Closed::Unknown;
            };
            let along = (apart * apart + radius * radius - far * far) / (2.0 * apart);
            let wide = radius * radius - along * along;
            if apart > radius + far - tol * 4.0
                || apart < (radius - far).abs() + tol * 4.0
                || wide <= 0.0
                || wide.sqrt() <= tol * 64.0
            {
                return Closed::Unknown;
            }
            Closed::Through(around(*centre + way * along, way, wide.sqrt()))
        }
        _ => Closed::Unknown,
    }
}

/// Whether the two touch in no single point where branches of their
/// crossing meet: where they touch, it is along a line, or in a point
/// nothing crosses at.
pub(crate) fn never_branch(a: &Known, b: &Known) -> bool {
    use Known::{Cylinder, Plane, Sphere};
    match (a, b) {
        (Plane { .. }, _) | (_, Plane { .. }) | (Sphere { .. }, Sphere { .. }) => true,
        (Cylinder { axis: da, .. }, Cylinder { axis: db, .. }) => da.cross(*db).length() <= 1e-12,
        _ => false,
    }
}
