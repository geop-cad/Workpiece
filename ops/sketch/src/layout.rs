//! The layout of a sketch's dimensions, in the sketch's (u, v)
//! coordinates: what a drawing has of one (thin extension and dimension
//! lines, slender filled arrowheads, the value written along the line).
//!
//! It is the one place a sketch dimension is laid out: the sketcher's
//! canvas draws what it returns, and the 3D view draws the very same
//! shapes mapped through the sketch plane's frame, so that a dimension
//! stands in the model as it does in the sketch. It is pure and has no
//! camera: the view comes in as sizes (a pixel's length in the plane)
//! and the way the screen's right and up point in the plane.

use shapecraft_core::handles::{CanvasShape, CanvasView, Tone};
use shapecraft_core::math::Point2;

use crate::draw::dimension_text;
use crate::geometry::{angle, unit};
use crate::model::{Constraint, Id, Kind, SketchArgs};

/// The width of the thin lines, in pixels.
pub(crate) const THIN: f32 = 1.0;
/// Extension lines start this far from what they measure, and pass the
/// dimension line by this much (in pixels).
const GAP: f64 = 3.0;
const OVERSHOOT: f64 = 7.0;
/// The length of an arrowhead; dimension lines shorter than three of
/// them get their arrows outside (in pixels).
pub const ARROW: f64 = 12.0;
/// How far a dimension's text sits above its line (in pixels).
const TEXT_LIFT: f64 = 9.0;

/// The shapes of the dimension `c` of `args` in `view`, in `tone`. None
/// of them has anything but finite coordinates, and none lies farther
/// from what is measured than the dimension's own label does (and a few
/// pixels).
pub fn dimension(
    args: &SketchArgs,
    c: &Constraint,
    view: &CanvasView,
    tone: Tone,
) -> Vec<CanvasShape> {
    let mut pen = Dim {
        out: Vec::new(),
        view,
    };
    pen.dimension(args, c, tone);
    pen.out
}

struct Dim<'a> {
    out: Vec<CanvasShape>,
    view: &'a CanvasView,
}

impl Dim<'_> {
    fn px(&self, pixels: f64) -> f64 {
        pixels * self.view.pixel
    }

    fn thin(&mut self, a: Point2, b: Point2, tone: Tone) {
        self.out.push(CanvasShape::Polyline {
            points: vec![a, b],
            tone,
            width: THIN,
            dashed: false,
        });
    }

    fn line(&mut self, points: Vec<Point2>, tone: Tone) {
        if points.len() >= 2 {
            self.out.push(CanvasShape::Polyline {
                points,
                tone,
                width: THIN,
                dashed: false,
            });
        }
    }

    fn arrow(&mut self, tip: Point2, direction: Point2, tone: Tone) {
        self.out.push(CanvasShape::Arrowhead {
            tip,
            direction,
            tone,
        });
    }

    /// The direction text along `along` is written in, so it reads from
    /// the bottom or from the right of the view.
    fn reading(&self, along: Point2) -> Point2 {
        let (right, up) = (along.dot(self.view.right), along.dot(self.view.up));
        if right < -1e-6 || (right.abs() <= 1e-6 && up < 0.0) {
            -along
        } else {
            along
        }
    }

    /// A dimension's text on the line through `at` along `along`, lifted
    /// above it.
    fn text(&mut self, at: Point2, along: Point2, constraint: &Constraint, tone: Tone) {
        let along = self.reading(along);
        let (text, upper, lower) = dimension_text(constraint);
        self.out.push(CanvasShape::Text {
            at: at + along.perp() * self.px(TEXT_LIFT),
            along,
            text,
            upper,
            lower,
            tone,
            badge: false,
        });
    }

    /// A straight dimension between `a` and `b`, measured along `m`
    /// (unit), its line through `label`.
    fn linear(
        &mut self,
        a: Point2,
        b: Point2,
        m: Point2,
        c: &Constraint,
        label: Point2,
        tone: Tone,
    ) {
        let n = m.perp();
        let lift = (label - a).dot(n);
        let (a1, length) = (a + n * lift, (b - a).dot(m));
        let b1 = a1 + m * length;
        // Extension lines, from just off the feature to just past the line.
        let side = if lift < 0.0 { -1.0 } else { 1.0 };
        for (from, to) in [(a, a1), (b, b1)] {
            if from.distance(to) > self.px(GAP) {
                self.thin(
                    from + n * side * self.px(GAP),
                    to + n * side * self.px(OVERSHOOT),
                    tone,
                );
            }
        }
        // The dimension line reaches out to the text if that is beside it.
        let at = (label - a1).dot(m);
        let (lo, hi) = (length.min(0.0), length.max(0.0));
        let outside = length.abs() < self.px(3.0 * ARROW);
        let reach = if outside { self.px(1.8 * ARROW) } else { 0.0 };
        let (from, to) = (lo.min(at) - reach, hi.max(at) + reach);
        self.thin(a1 + m * from, a1 + m * to, tone);
        let out = if (length < 0.0) != outside { m } else { -m };
        self.arrow(a1, out, tone);
        self.arrow(b1, -out, tone);
        self.text(a1 + m * at, m, c, tone);
    }

    fn dimension(&mut self, args: &SketchArgs, c: &Constraint, tone: Tone) {
        let Some(d) = &c.dimension else {
            return;
        };
        let label = d.label;
        match c.kind {
            Kind::Distance(a, b) => {
                let (a, b) = (args.at(a), args.at(b));
                let m = (b - a).try_normalize().unwrap_or(Point2::X);
                self.linear(a, b, m, c, label, tone);
            }
            Kind::DistanceAlong { a, b, axis } => {
                self.linear(args.at(a), args.at(b), axis.unit(), c, label, tone);
            }
            Kind::PointLine { point, line } => {
                let (Some((a, b)), p) = (args.line(line), args.at(point)) else {
                    return;
                };
                let along = (b - a).try_normalize().unwrap_or(Point2::X);
                let foot = a + along * (p - a).dot(along);
                let m = (p - foot).try_normalize().unwrap_or(along.perp());
                // A diameter: from the point's mirror image in the axis
                // to the point.
                let from = if d.diameter { foot * 2.0 - p } else { foot };
                self.linear(from, p, m, c, label, tone);
            }
            Kind::Size(e) => {
                let Some((_, center, r)) = args.circle(e) else {
                    return;
                };
                let u = (label - center).try_normalize().unwrap_or(Point2::X);
                let reach = (label - center).length();
                let on = center + u * r;
                if d.diameter {
                    // Across the circle, an arrow at each end.
                    self.thin(center - u * r, center + u * reach.max(r), tone);
                    self.arrow(on, u, tone);
                    self.arrow(center - u * r, -u, tone);
                } else {
                    // From the center to the arc, or from outside onto it.
                    self.thin(center, center + u * reach.max(r), tone);
                    self.arrow(on, if reach > r { -u } else { u }, tone);
                }
                self.text(label, u, c, tone);
            }
            Kind::Angle(l1, l2) => self.angle(args, l1, l2, c, tone),
            Kind::Offset { a, b, .. } => {
                if let (Some((a1, a2)), Some((b1, _))) = (args.line(a), args.line(b)) {
                    // Between the lines, like a point's distance from one.
                    let along = (a2 - a1).try_normalize().unwrap_or(Point2::X);
                    let foot = a1 + along * (b1 - a1).dot(along);
                    let m = (b1 - foot).try_normalize().unwrap_or(along.perp());
                    self.linear(foot, b1, m, c, label, tone);
                } else if let (Some((_, center, r1)), Some((_, _, r2))) =
                    (args.circle(a), args.circle(b))
                {
                    // Between the circles, along the radius through the text.
                    let u = (label - center).try_normalize().unwrap_or(Point2::X);
                    self.linear(center + u * r1, center + u * r2, u, c, label, tone);
                }
            }
            _ => {}
        }
    }

    /// An angle: an arc about where the lines meet, through the text.
    fn angle(&mut self, args: &SketchArgs, l1: Id, l2: Id, c: &Constraint, tone: Tone) {
        let (Some((a1, b1)), Some((a2, b2)), Some(d)) =
            (args.line(l1), args.line(l2), c.dimension.as_ref())
        else {
            return;
        };
        let (d1, d2) = (b1 - a1, b2 - a2);
        let det = d1.perp_dot(d2);
        if det.abs() < 1e-12 {
            self.text(d.label, Point2::X, c, tone);
            return;
        }
        let meet = a1 + d1 * ((a2 - a1).perp_dot(d2) / det);
        let radius = meet.distance(d.label).max(self.px(2.0 * ARROW));
        let total = d.value.to_radians();
        // The sector from the first line's direction, or the one across
        // from it if the text is there.
        let mut from = angle(d1);
        let to_label = (angle(d.label - meet) - from).rem_euclid(std::f64::consts::TAU);
        if to_label > total
            && to_label >= std::f64::consts::PI
            && to_label <= std::f64::consts::PI + total
        {
            from += std::f64::consts::PI;
        }
        let steps = ((total.abs() / 0.05).ceil() as usize).max(2);
        let arc: Vec<Point2> = (0..=steps)
            .map(|k| meet + unit(from + total * k as f64 / steps as f64) * radius)
            .collect();
        let (start, end) = (arc[0], arc[steps]);
        self.line(arc, tone);
        self.arrow(start, -unit(from).perp(), tone);
        self.arrow(end, unit(from + total).perp(), tone);
        // Extension lines where the arc ends beyond a line.
        for ((a, b), end) in [((a1, b1), start), ((a2, b2), end)] {
            let dir = (end - meet).normalize_or_zero();
            let (sa, sb) = ((a - meet).dot(dir), (b - meet).dot(dir));
            let (lo, hi) = (sa.min(sb), sa.max(sb));
            if radius > hi {
                self.thin(
                    meet + dir * (hi + self.px(GAP)),
                    meet + dir * (radius + self.px(OVERSHOOT)),
                    tone,
                );
            } else if radius < lo {
                self.thin(
                    meet + dir * (lo - self.px(GAP)),
                    meet + dir * (radius - self.px(OVERSHOOT)),
                    tone,
                );
            }
        }
        let radial = (d.label - meet).try_normalize().unwrap_or(Point2::X);
        self.text(meet + radial * radius, radial.perp(), c, tone);
    }
}

/// The dimension `of` an annotation says a sketch's, laid out as the
/// sketcher lays it out: `None` for a sketch it has not the geometry of.
pub fn annotation(
    of: &shapecraft_core::world::SketchOf,
    view: &CanvasView,
    tone: Tone,
) -> Option<Vec<CanvasShape>> {
    let args = of.geometry.get::<SketchArgs>()?;
    let constraint = args.constraints.get(&of.constraint)?;
    Some(dimension(args, constraint, view, tone))
}
