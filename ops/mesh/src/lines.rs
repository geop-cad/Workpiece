//! The curves a mesh's lines make: each run of lines that go on from
//! one another, its points the control polygon of a curve — as a face
//! cage controls its surface. Smooth, the curve is the limit of the
//! run's subdivision (a uniform cubic B-spline over its points; an open
//! end goes through its point, a closed run is smooth all round, a
//! corner is a sharp point it goes through); flat, the run itself.

use std::collections::BTreeSet;

use anyhow::Context;
use workpiece_core::math::Point3;
use workpiece_core::world::topo::{Body, BodyKind, Curve3, Edge, Topo, Vertex};

use crate::poly::Poly;

/// The runs of `poly`'s lines, each its points in order and whether it
/// closes: broken where more than two lines meet and at the corners.
pub fn runs(poly: &Poly) -> Vec<(Vec<usize>, bool)> {
    let mut out = Vec::new();
    for (run, closed) in Poly::runs(&poly.lines) {
        let corner = |v: &usize| poly.corners.contains(v);
        if closed {
            // From a corner round to it again, open at it.
            match run.iter().position(corner) {
                None => out.push((run, true)),
                Some(k) => {
                    let mut turned: Vec<usize> =
                        run[k..].iter().chain(&run[..k]).copied().collect();
                    turned.push(run[k]);
                    out.extend(split(&turned, &poly.corners));
                }
            }
        } else {
            out.extend(split(&run, &poly.corners));
        }
    }
    out
}

/// The open run `run` cut at its inner points that are `corners`.
fn split(run: &[usize], corners: &BTreeSet<usize>) -> Vec<(Vec<usize>, bool)> {
    let mut out = Vec::new();
    let mut piece = vec![run[0]];
    for (k, &v) in run.iter().enumerate().skip(1) {
        piece.push(v);
        if k + 1 < run.len() && corners.contains(&v) {
            out.push((std::mem::replace(&mut piece, vec![v]), false));
        }
    }
    out.push((piece, false));
    out
}

/// The limit curve of the subdivision of the control polygon `points`:
/// a uniform cubic B-spline over them, as a clamped one (it starts and
/// ends at a knot of multiplicity four). Open, its ends go through the
/// first and the last point (the subdivision keeps them where they
/// are); closed, it runs on smooth through the first.
pub fn limit(points: &[Point3], closed: bool) -> anyhow::Result<Curve3> {
    let n = points.len();
    anyhow::ensure!(
        n >= 2 && (!closed || n >= 3),
        "a curve of lines has two points or more"
    );
    // The uniform B-spline's control points: the points with one more
    // at each end that the subdivision's rule there makes of them —
    // mirrored through the end, or the points round the loop.
    let c: Vec<Point3> = if closed {
        let mut c = vec![points[n - 1]];
        c.extend_from_slice(points);
        c.extend([points[0], points[1]]);
        c
    } else {
        let mut c = vec![points[0] * 2.0 - points[1]];
        c.extend_from_slice(points);
        c.push(points[n - 1] * 2.0 - points[n - 2]);
        c
    };
    // Clamped at both ends (Boehm's knot insertion, twice at each).
    let m = c.len() - 1;
    let segments = m - 2;
    let mut control = vec![(c[0] + c[1] * 4.0 + c[2]) / 6.0, (c[1] * 2.0 + c[2]) / 3.0];
    control.extend_from_slice(&c[2..m - 1]);
    control.push((c[m - 1] * 2.0 + c[m - 2]) / 3.0);
    control.push((c[m - 2] + c[m - 1] * 4.0 + c[m]) / 6.0);
    let mut knots = vec![0.0; 4];
    knots.extend((1..segments).map(|k| k as f64));
    knots.extend([segments as f64; 4]);
    Ok(Curve3::new(3, knots, control, None)?)
}

/// The run `points` as it is: straight from point to point.
pub fn polyline(points: &[Point3]) -> anyhow::Result<Curve3> {
    let n = points.len();
    anyhow::ensure!(n >= 2, "a line has two points");
    let mut knots = vec![0.0; 2];
    knots.extend((1..n - 1).map(|k| k as f64));
    knots.extend([(n - 1) as f64; 2]);
    Ok(Curve3::new(1, knots, points.to_vec(), None)?)
}

/// The curves of `poly`'s lines: smooth (the limit of their
/// subdivision) or straight.
pub fn curves(poly: &Poly, smooth: bool) -> anyhow::Result<Vec<Curve3>> {
    runs(poly)
        .into_iter()
        .map(|(run, closed)| {
            let mut at: Vec<Point3> = run.iter().map(|&v| poly.points[v]).collect();
            if smooth {
                limit(&at, closed)
            } else {
                if closed {
                    at.push(at[0]);
                }
                polyline(&at)
            }
        })
        .collect::<anyhow::Result<_>>()
        .context("the curves of the lines")
}

/// Curves as a wire body: each an edge, `{name}/e0`, `{name}/e1`, …;
/// ends at one place are one vertex.
pub struct Wire(pub Vec<Curve3>);

impl workpiece_core::Shape for Wire {
    fn write(&self, topo: &mut Topo, name: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!self.0.is_empty(), "there is no curve");
        let size = self
            .0
            .iter()
            .flat_map(|c| c.control_points().iter())
            .fold(0.0_f64, |m, p| m.max(p.length()))
            .max(1.0);
        let tolerance = size * 1e-9;
        let mut vertices: Vec<(Point3, workpiece_core::world::topo::VertexId)> = Vec::new();
        let mut edges = Vec::new();
        for (k, curve) in self.0.iter().enumerate() {
            let (t0, t1) = curve.domain();
            let mut at = |topo: &mut Topo, p: Point3| -> anyhow::Result<_> {
                if let Some((_, v)) = vertices
                    .iter()
                    .find(|(q, _)| q.distance(p) <= tolerance * 100.0)
                {
                    return Ok(*v);
                }
                let v = topo.add_vertex(Vertex {
                    point: p,
                    tolerance: tolerance * 100.0,
                })?;
                vertices.push((p, v));
                Ok(v)
            };
            let start = at(topo, curve.point_at(t0))?;
            let end = at(topo, curve.point_at(t1))?;
            let id = topo.add_curve(curve.clone());
            let edge = topo.add_edge(Edge {
                curve: id,
                t0,
                t1,
                start,
                end,
                tolerance,
            })?;
            topo.set_name(edge, format!("{name}/e{k}"))?;
            edges.push(edge);
        }
        let body = topo.add_body(Body {
            kind: BodyKind::Wire,
            lumps: Vec::new(),
            free_edges: edges,
            free_vertices: Vec::new(),
        })?;
        topo.set_name(body, name)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_run_ends_at_its_points_and_follows_the_subdivision() -> anyhow::Result<()> {
        let p = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(3.0, 3.0, 0.0),
            Point3::new(6.0, 3.0, 1.0),
        ];
        let curve = limit(&p, false)?;
        assert_eq!(curve.degree(), 3);
        let (t0, t1) = curve.domain();
        assert!(curve.point_at(t0).abs_diff_eq(p[0], 1e-12));
        assert!(curve.point_at(t1).abs_diff_eq(p[3], 1e-12));
        // The control points: the ends, a third of the way to the next,
        // and the inner points themselves.
        let want = [
            p[0],
            (p[0] * 2.0 + p[1]) / 3.0,
            p[1],
            p[2],
            (p[2] + p[3] * 2.0) / 3.0,
            p[3],
        ];
        let got = curve.control_points();
        assert_eq!(got.len(), want.len());
        for (a, b) in got.iter().zip(want) {
            assert!(a.abs_diff_eq(b, 1e-12));
        }
        // Where an inner point's piece ends: the limit of that point,
        // (one, six, one) of it and those beside it over eight — after
        // the rule's steps, a sixth, four sixths, a sixth.
        let at = curve.point_at(1.0);
        assert!(at.abs_diff_eq((p[0] + p[1] * 4.0 + p[2]) / 6.0, 1e-12));
        Ok(())
    }

    #[test]
    fn a_closed_run_closes_smooth() -> anyhow::Result<()> {
        let p = [
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(0.0, -1.0, 0.0),
        ];
        let curve = limit(&p, true)?;
        let (t0, t1) = curve.domain();
        let (a, b) = (curve.point_at(t0), curve.point_at(t1));
        assert!(a.abs_diff_eq(b, 1e-12), "it closes");
        assert!(a.abs_diff_eq((p[3] + p[0] * 4.0 + p[1]) / 6.0, 1e-12));
        // The same way on at the seam: smooth there.
        let d = 1e-6;
        let leaving = (curve.point_at(t0 + d) - a).normalize();
        let arriving = (b - curve.point_at(t1 - d)).normalize();
        assert!(leaving.abs_diff_eq(arriving, 1e-5));
        Ok(())
    }
}
