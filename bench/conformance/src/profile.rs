//! A profile of the suite, given as the kernel's own: drawn in a sketch,
//! or as curves in space. Only the form changes: a line is a line, an
//! arc an arc, a conic the rational quadratic it is defined as, and a
//! periodic spline the same curve with its knots clamped (by knot
//! insertion, which changes no point of it).

use std::f64::consts::TAU;
use workpiece_core::math::from_angle;

use serde_json::Value;
use workpiece_core::math::Point2;
use workpiece_core::world::topo::Curve3;
use workpiece_ops_sketch::{Shape, SketchArgs};

use crate::{Answer, Frame, No, malformed};

/// A segment of a loop.
pub enum Segment {
    /// From the first point straight to the second.
    Line(Point2, Point2),
    /// Around `center` from the angle `start` through `sweep`.
    Arc {
        /// The circle's centre.
        center: Point2,
        /// Its radius.
        radius: f64,
        /// The angle it starts at.
        start: f64,
        /// The angle it turns through, counter-clockwise if positive.
        sweep: f64,
    },
    /// The rational quadratic from `a` to `b` with the corner `apex` of
    /// weight `w`.
    Conic {
        /// Where it starts.
        a: Point2,
        /// Its corner.
        apex: Point2,
        /// Where it ends.
        b: Point2,
        /// The corner's weight.
        w: f64,
    },
    /// A closed spline, clamped: its degree, knots and control points
    /// (the last where the first is).
    Spline(usize, Vec<f64>, Vec<Point2>),
    /// A rational curve of degree 5 between its first control point and
    /// its last: the control points, and their weights.
    Blend(Vec<Point2>, Vec<f64>),
}

fn point2(v: Option<&Value>) -> Answer<Point2> {
    let a = v.and_then(Value::as_array).filter(|a| a.len() == 2);
    let a = a.ok_or_else(|| malformed("a point of a profile"))?;
    match (a[0].as_f64(), a[1].as_f64()) {
        (Some(x), Some(y)) => Ok(Point2::new(x, y)),
        _ => Err(malformed("a coordinate of a profile")),
    }
}

fn number(v: &Value, key: &str) -> Answer<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| malformed(key))
}

/// A periodic B-spline as the suite writes it — the points of one
/// period and the first `degree` of them again, over knots that run
/// past both ends of the period — as the same curve over clamped knots:
/// the knots at the period's ends are inserted until the curve starts
/// and ends at a control point.
fn clamped(degree: usize, mut knots: Vec<f64>, mut ctrl: Vec<Point2>) -> Answer<Segment> {
    let p = degree;
    if p == 0 || ctrl.len() <= p || knots.len() != ctrl.len() + p + 1 {
        return Err(malformed("a spline's knots and control points"));
    }
    let (a, b) = (knots[p], knots[ctrl.len()]);
    for u in [a, b] {
        loop {
            let times = knots.iter().filter(|k| **k == u).count();
            if times >= p {
                break;
            }
            // The span the knot falls in: knots[k] <= u < knots[k + 1].
            let k = knots.iter().rposition(|x| *x <= u).unwrap_or(0);
            let mut next = Vec::with_capacity(ctrl.len() + 1);
            for i in 0..=ctrl.len() {
                // (At a knot there already, the last shares are nothing:
                // those points stay the ones before them.)
                next.push(if i + p <= k {
                    ctrl[i]
                } else if i > k || knots[i] == u {
                    ctrl[i - 1]
                } else {
                    let share = (u - knots[i]) / (knots[i + p] - knots[i]);
                    ctrl[i - 1] * (1.0 - share) + ctrl[i] * share
                });
            }
            ctrl = next;
            knots.insert(k + 1, u);
        }
    }
    let first = knots.iter().position(|k| *k == a).unwrap_or(0);
    let last = knots.iter().position(|k| *k == b).unwrap_or(0);
    if first == 0 || last <= first {
        return Err(malformed("a spline's period"));
    }
    let mut clamped = vec![a];
    clamped.extend_from_slice(&knots[first..last + p]);
    clamped.push(b);
    let mut points = ctrl[first - 1..last].to_vec();
    // (It closes: the two ends are one point, but for rounding.)
    let start = points[0];
    if let Some(end) = points.last_mut() {
        *end = start;
    }
    Ok(Segment::Spline(p, clamped, points))
}

/// The knots of a curve of degree 5 in one piece.
const BLEND_KNOTS: [f64; 12] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];

/// The loops of a profile.
pub fn loops(v: &Value) -> Answer<Vec<Vec<Segment>>> {
    let all = v.as_array().ok_or_else(|| malformed("a profile"))?;
    let mut loops = Vec::new();
    for l in all {
        let segments = l.as_array().ok_or_else(|| malformed("a loop"))?;
        let mut made = Vec::new();
        for s in segments {
            if let Some(line) = s.get("line") {
                let ends = line.as_array().filter(|a| a.len() == 2);
                let ends = ends.ok_or_else(|| malformed("a line"))?;
                made.push(Segment::Line(
                    point2(Some(&ends[0]))?,
                    point2(Some(&ends[1]))?,
                ));
            } else if let Some(arc) = s.get("arc") {
                made.push(Segment::Arc {
                    center: point2(arc.get("center"))?,
                    radius: number(arc, "radius")?,
                    start: number(arc, "start")?,
                    sweep: number(arc, "sweep")?,
                });
            } else if let Some(conic) = s.get("conic") {
                made.push(Segment::Conic {
                    a: point2(conic.get("a"))?,
                    apex: point2(conic.get("apex"))?,
                    b: point2(conic.get("b"))?,
                    w: number(conic, "w")?,
                });
            } else if let Some(spline) = s.get("spline") {
                let degree = spline.get("degree").and_then(Value::as_u64);
                let degree = degree.ok_or_else(|| malformed("degree"))? as usize;
                let knots = spline.get("knots").and_then(Value::as_array);
                let knots: Option<Vec<f64>> = knots
                    .ok_or_else(|| malformed("knots"))?
                    .iter()
                    .map(Value::as_f64)
                    .collect();
                let ctrl = spline.get("ctrl").and_then(Value::as_array);
                let ctrl: Answer<Vec<Point2>> = ctrl
                    .ok_or_else(|| malformed("ctrl"))?
                    .iter()
                    .map(|p| point2(Some(p)))
                    .collect();
                made.push(clamped(
                    degree,
                    knots.ok_or_else(|| malformed("knots"))?,
                    ctrl?,
                )?);
            } else if let Some(gn) = s.get("gn") {
                if gn.get("order").and_then(Value::as_u64) != Some(2) {
                    return Err(No::Unsupported(
                        "a blend section of another order than 2".into(),
                    ));
                }
                // The kernel's own section of that kind: a rational
                // curve of degree 5.
                let (a, apex, b) = (
                    point2(gn.get("a"))?,
                    point2(gn.get("apex"))?,
                    point2(gn.get("b"))?,
                );
                let lift = |p: Point2| workpiece_core::math::Point3::new(p.x, p.y, 0.0);
                let all =
                    workpiece_shape::gn::section(lift(a), lift(apex), lift(b), number(gn, "w")?);
                let mut points: Vec<Point2> = all.iter().map(|p| Point2::new(p[0], p[1])).collect();
                (points[0], points[5]) = (a, b);
                made.push(Segment::Blend(points, all.iter().map(|p| p[3]).collect()));
            } else {
                return Err(No::Unsupported("a segment of a kind not known".into()));
            }
        }
        if made.is_empty() {
            return Err(malformed("a loop without segments"));
        }
        loops.push(made);
    }
    Ok(loops)
}

impl Segment {
    fn ends(&self) -> (Point2, Point2) {
        match self {
            Segment::Line(a, b) => (*a, *b),
            Segment::Arc {
                center,
                radius,
                start,
                sweep,
            } => (
                *center + from_angle(*start) * *radius,
                *center + from_angle(start + sweep) * *radius,
            ),
            Segment::Conic { a, b, .. } => (*a, *b),
            Segment::Spline(_, _, points) | Segment::Blend(points, _) => {
                (points[0], points[points.len() - 1])
            }
        }
    }

    /// Whether it is all of a circle.
    fn whole(&self) -> bool {
        matches!(self, Segment::Arc { sweep, .. } if (sweep.abs() - TAU).abs() <= 1e-12)
    }
}

/// Draws the loops in a sketch: each segment from the point the one
/// before it ended at, the last back to the first's start.
pub fn draw(loops: &[Vec<Segment>], sketch: &mut SketchArgs) -> Answer<()> {
    for segments in loops {
        if let [only] = segments.as_slice() {
            match only {
                Segment::Arc { center, radius, .. } if only.whole() => {
                    let center = sketch.add_point(*center);
                    sketch.add(Shape::Circle {
                        center,
                        radius: *radius,
                    });
                    continue;
                }
                Segment::Spline(degree, knots, points) => {
                    let mut control: Vec<_> = points[..points.len() - 1]
                        .iter()
                        .map(|p| sketch.add_point(*p))
                        .collect();
                    control.push(control[0]);
                    sketch.add(Shape::Spline {
                        control,
                        degree: *degree,
                        knots: knots.clone(),
                        weights: Vec::new(),
                    });
                    continue;
                }
                _ => {}
            }
        }
        let first = sketch.add_point(segments[0].ends().0);
        let mut from = first;
        for (k, segment) in segments.iter().enumerate() {
            let to = if k + 1 == segments.len() {
                first
            } else {
                sketch.add_point(segment.ends().1)
            };
            match segment {
                Segment::Line(..) => {
                    sketch.add_line(from, to);
                }
                Segment::Arc { center, sweep, .. } => {
                    let center = sketch.add_point(*center);
                    // The sketch's arcs run counter-clockwise.
                    let (start, end) = if *sweep >= 0.0 {
                        (from, to)
                    } else {
                        (to, from)
                    };
                    sketch.add(Shape::Arc { center, start, end });
                }
                Segment::Conic { apex, w, .. } => {
                    let apex = sketch.add_point(*apex);
                    sketch.add(Shape::Spline {
                        control: vec![from, apex, to],
                        degree: 2,
                        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                        weights: vec![1.0, *w, 1.0],
                    });
                }
                Segment::Blend(points, weights) => {
                    let mut control = vec![from];
                    control.extend(points[1..5].iter().map(|p| sketch.add_point(*p)));
                    control.push(to);
                    sketch.add(Shape::Spline {
                        control,
                        degree: 5,
                        knots: BLEND_KNOTS.to_vec(),
                        weights: weights.clone(),
                    });
                }
                Segment::Spline(..) => {
                    return Err(malformed("a spline is the whole of its loop"));
                }
            }
            from = to;
        }
    }
    Ok(())
}

/// The loop as curves in space: in the frame's xy plane, `height` up its
/// z, one after the other.
pub fn curves(segments: &[Segment], frame: &Frame, height: f64) -> Answer<Vec<Curve3>> {
    let place = |p: Point2| frame.place(p.x, p.y, height);
    let [x, y, _] = frame.axes;
    let mut all = Vec::new();
    for segment in segments {
        all.push(match segment {
            Segment::Line(a, b) => Curve3::line(place(*a), place(*b))?,
            Segment::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let (sin, cos) = libm::sincos(*start);
                let out = (x * cos + y * sin) * *radius;
                let on = (y * cos - x * sin) * *radius * sweep.signum();
                workpiece_shape::arcs::ellipse(place(*center), out, on, sweep.abs())
            }
            Segment::Conic { a, apex, b, w } => Curve3::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![place(*a), place(*apex), place(*b)],
                Some(vec![1.0, *w, 1.0]),
            )?,
            Segment::Blend(points, weights) => Curve3::new(
                5,
                BLEND_KNOTS.to_vec(),
                points.iter().map(|p| place(*p)).collect(),
                Some(weights.clone()),
            )?,
            Segment::Spline(degree, knots, points) => Curve3::new(
                *degree,
                knots.clone(),
                points.iter().map(|p| place(*p)).collect(),
                None,
            )?,
        });
    }
    Ok(all)
}
