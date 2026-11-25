//! Threads that are said of the model, not cut into it, the way
//! drawings show them (ISO 6410-1): seen from the side, a narrow line
//! along the roots (inside the outline of a screw's thread, outside
//! the bore of a nut's) and a wide one across where the thread ends;
//! seen along the axis, three quarters of a narrow circle at the
//! roots. Hidden, all of it is dashed. In a section the hatching runs
//! to the crests — the face the thread is on — and so across the
//! narrow lines of a nut's thread.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use shapecraft_core::math::{Point2, Point3};
use shapecraft_core::world::{Cutting, Thread};

use super::{Laid, Pen};
use crate::sheet::Style;

/// What a view sees of the threads, in the view's own coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Threads {
    /// The lines along the roots of threads seen from the side, each
    /// with whether the model hides it.
    pub roots: Vec<(Point2, Point2, bool)>,
    /// The lines across where threads seen from the side end, each
    /// with whether the model hides it.
    pub ends: Vec<(Point2, Point2, bool)>,
    /// The circles at the roots of threads seen along their axes —
    /// three quarters of each are drawn: middle and radius, each with
    /// whether the model hides it.
    pub rounds: Vec<(Point2, f64, bool)>,
}

/// Whether the cut `cut` takes away the point `p`: it is to the left
/// of the cutting line, by more than `near`.
fn taken(cut: &Cutting, p: Point3, near: f64) -> bool {
    let d = p - cut.origin;
    let p = Point2::new(d.dot(cut.x), d.dot(cut.y));
    let pieces = cut.path.len() - 1;
    // The piece of the line it is beside (the first and the last run
    // on).
    let beside = cut
        .path
        .windows(2)
        .enumerate()
        .map(|(k, w)| {
            let along = w[1] - w[0];
            let s = (p - w[0]).dot(along) / along.length_squared().max(1e-300);
            let s = s.clamp(
                if k == 0 { f64::NEG_INFINITY } else { 0.0 },
                if k + 1 == pieces { f64::INFINITY } else { 1.0 },
            );
            let off = (w[0] + along * s).distance(p);
            (off, along.normalize_or_zero().perp_dot(p - w[0]))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    beside.is_some_and(|(_, left)| left > near)
}

/// What is seen of `threads` looking against `towards`: `flat` puts a
/// point of the model into the view, `behind` says whether the model
/// hides one, `cut` is where the view cuts the model open, and `reach`
/// how large the model is.
pub(crate) fn see(
    threads: &BTreeMap<String, Thread>,
    towards: Point3,
    flat: &dyn Fn(Point3) -> Point2,
    behind: &dyn Fn(Point3) -> bool,
    cut: Option<&Cutting>,
    reach: f64,
) -> Threads {
    let mut seen = Threads::default();
    let nudge = reach * 4e-3;
    let gone = |p: Point3| cut.is_some_and(|cut| taken(cut, p, reach * 1e-7));
    for thread in threads.values() {
        let (from, to) = (thread.origin, thread.origin + thread.axis * thread.length);
        let facing = thread.axis.dot(towards);
        if facing.abs() > 1.0 - 1e-9 {
            // Along its axis: looked at from its nearer end — on the
            // end of a rod, into the mouth of a hole. Cut open across,
            // it is seen where it is cut.
            let (near, far) = if facing > 0.0 { (to, from) } else { (from, to) };
            let hidden = if gone(near) {
                if gone(far) {
                    continue;
                }
                false
            } else {
                let aside = if thread.internal {
                    Point3::ZERO
                } else {
                    thread.axis.any_orthonormal_vector() * (thread.minor / 4.0)
                };
                behind(near + aside + towards * nudge)
            };
            seen.rounds
                .push((flat(thread.origin), thread.roots() / 2.0, hidden));
        } else if facing.abs() < 1e-9 {
            // From the side: a line along the roots at either side.
            // Each is in view where the outline of the face beside it
            // is: looked for a little off the face, outside a rod and
            // inside a hole.
            let aside = thread.axis.cross(towards).normalize_or_zero();
            let mut hides = [false; 2];
            let mut drawn = false;
            for (k, side) in [1.0, -1.0].into_iter().enumerate() {
                let off = if thread.internal { -nudge } else { nudge };
                let outline = (from + to) / 2.0 + aside * (side * (thread.crests() / 2.0 + off));
                if gone(outline) {
                    continue;
                }
                let roots = aside * (side * thread.roots() / 2.0);
                hides[k] = behind(outline);
                drawn = true;
                seen.roots
                    .push((flat(from + roots), flat(to + roots), hides[k]));
            }
            if drawn {
                let across = aside * (thread.roots() / 2.0);
                let hidden = hides[0] && hides[1];
                for end in [from, to] {
                    seen.ends
                        .push((flat(end + across), flat(end - across), hidden));
                }
            }
        }
        // (Seen askew, a thread that is not modelled has no way of
        // being drawn that the standard knows.)
    }
    seen
}

/// Draws `threads`: `sheet` puts a point of the view onto the sheet,
/// at `scale` millimetres a length of the model; `clip` is the part of
/// a line the view shows.
pub(crate) fn draw(
    laid: &mut Laid,
    threads: &Threads,
    style: Style,
    scale: f64,
    sheet: &dyn Fn(Point2) -> Point2,
    clip: &dyn Fn(Point2, Point2) -> Option<(Point2, Point2)>,
) {
    let pen = |hidden: bool, pen: Pen| match (style, hidden) {
        (Style::Wireframe, _) | (_, false) => Some(pen),
        (Style::HiddenDashed, true) => Some(Pen::Dashed),
        (_, true) => None,
    };
    let lines = threads
        .roots
        .iter()
        .map(|l| (l, Pen::Narrow))
        .chain(threads.ends.iter().map(|l| (l, Pen::Wide)));
    for ((a, b, hidden), with) in lines {
        if let (Some(pen), Some((a, b))) = (pen(*hidden, with), clip(*a, *b)) {
            laid.line(sheet(a), sheet(b), pen);
        }
    }
    // Three quarters of a circle: open at the upper right, and a
    // little past the centre lines at either end.
    let (start, sweep) = (FRAC_PI_2 + 0.12, TAU * 0.75 - 0.24);
    for (at, radius, hidden) in &threads.rounds {
        let Some(pen) = pen(*hidden, Pen::Narrow) else {
            continue;
        };
        let steps = ((sweep * radius * scale / 1.5).ceil() as usize).clamp(12, 180);
        let on =
            |k: usize| *at + Point2::from_angle(start + sweep * k as f64 / steps as f64) * *radius;
        for k in 0..steps {
            if let Some((a, b)) = clip(on(k), on(k + 1)) {
                laid.line(sheet(a), sheet(b), pen);
            }
        }
    }
}
