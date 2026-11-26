//! What is said of the model, drawn in the views of a sheet: dimensions
//! (ISO 129-1), datum features (ISO 5459), geometric tolerances (ISO
//! 1101) — and which of it stands nowhere yet.

use crate::Sheet;
use workpiece_core::math::Point2;
use workpiece_core::math::from_angle;
use workpiece_core::world::annotation::number;
use workpiece_core::world::{Annotation, Callout, Characteristic, Measured, Note, Tolerance};

use super::views::Placed;
use super::{Laid, LaidNote, Pen, Subject, lettering};
use workpiece_core::math::Point3;

/// How something looks at the model and lays what it sees flat: a view
/// of a sheet, or the plane a note lies in on the model itself.
pub(crate) trait Frame {
    /// Where the point `p` of the model is, in millimetres.
    fn place(&self, p: Point3) -> Point2;
    /// The direction `d` of the model as it is seen: across and up, in
    /// the model's lengths.
    fn way(&self, d: Point3) -> Point2;
    /// How many millimetres one length of the model is.
    fn scale(&self) -> f64;
    /// The way to who looks (a unit vector of the model).
    fn towards(&self) -> Point3;
    /// Whether it shows the model whole (a detail does not).
    fn whole(&self) -> bool {
        true
    }
    /// Whether the end `x` (on the sheet) of a length lies off what it
    /// shows: outside a detail's circle (an end slid across to its
    /// middle), in the half of a half view or section that is not drawn
    /// or where what it measures is not seen. A length with one end so
    /// is drawn as a half dimension (ISO 129-1).
    fn cut_off(&self, _x: Point2) -> bool {
        false
    }
    /// How far from `a` (on the sheet) the way `toward` (a unit vector)
    /// what it shows reaches: to a detail's circle, to the middle line
    /// of a half view. Nothing where it shows all that way.
    fn shown_to(&self, _a: Point2, _toward: Point2) -> Option<f64> {
        None
    }
    /// Of a detail, the circle on the sheet it shows what is in:
    /// its middle and radius. Nothing for a view of all of the model.
    fn circle(&self) -> Option<(Point2, f64)> {
        None
    }
    /// Where an extension line from `p` (on the sheet) that runs on to
    /// `foot` (where it meets its dimension line) starts on what is
    /// drawn: see [`Outline`]. Nothing is known of what is drawn where
    /// this is not a view.
    fn outline(&self, _p: Point2, _foot: Point2) -> Outline {
        Outline::Free
    }
    /// Of what is round about `c` (on the sheet) with the radius `r`
    /// (in millimetres): the point of its arc as drawn nearest to where
    /// the way `way` (a unit vector) from `c` meets it — where an
    /// arrowhead of its radius or diameter can stand. Nothing where
    /// nothing that round is drawn (a construction circle); where
    /// nothing is known of what is drawn, the point `way` points at.
    fn on_arc(&self, c: Point2, r: f64, way: Point2) -> Option<Point2> {
        Some(c + way * r)
    }
    /// How far along `way` (a unit vector) from `p` the lines drawn
    /// along the line through `p` that runs that way reach: the least
    /// and the most. Nothing where none is drawn, or nothing is known
    /// of what is drawn.
    fn along(&self, _p: Point2, _way: Point2) -> Option<(f64, f64)> {
        None
    }
    /// The stretches of the line through `p` along `way` (a unit
    /// vector) that the view draws lines along — how far from `p`, each
    /// from and to, in order: where a face seen edge on is drawn, and
    /// not where a hole in it leaves a gap. Nothing where none is drawn,
    /// or nothing is known of what is drawn.
    fn drawn_along(&self, _p: Point2, _way: Point2) -> Vec<(f64, f64)> {
        Vec::new()
    }
    /// The circles the view draws of what it sees round and square on
    /// (the bores and shafts it looks along): middle and radius, on the
    /// sheet.
    fn circles(&self) -> Vec<(Point2, f64)> {
        Vec::new()
    }
    /// Of the lines the view draws, the point nearest to lying `r` from
    /// `c` the way `way` (a unit vector) points, within half of `r` of
    /// that: where a radius of a round the view does not draw as a
    /// circle (a round a section cuts slantwise) points at the outline.
    /// Nothing where there is none, or nothing is known of what is
    /// drawn.
    fn near_round(&self, _c: Point2, _r: f64, _way: Point2) -> Option<Point2> {
        None
    }

    /// Of an arc about `c` (on the sheet), `r` millimetres large, that
    /// the view hides and does not draw (a round behind something
    /// nearer), its point nearest the way `way` from `c`; nothing where
    /// there is no such arc.
    fn hidden_arc(&self, _c: Point2, _r: f64, _way: Point2) -> Option<Point2> {
        None
    }
}

/// The lines the view `placed` draws, on the sheet: in view, and the
/// hidden ones where it draws those.
fn drawn_lines(placed: &Placed<'_>) -> Vec<(Point2, Point2)> {
    use crate::sheet::{Style, TangentEdges};
    if placed.only_cut() {
        return placed
            .cut_outline()
            .into_iter()
            .map(|(a, b)| (placed.sheet(a), placed.sheet(b)))
            .collect();
    }
    let style = placed.view.style;
    placed
        .seen
        .lines
        .iter()
        .filter(|(_, _, behind, smooth)| {
            (!behind || matches!(style, Style::HiddenDashed | Style::Wireframe))
                && !(*smooth && (*behind || placed.tangent == TangentEdges::Hidden))
        })
        .map(|(a, b, _, _)| (placed.sheet(*a), placed.sheet(*b)))
        .collect()
}

/// Where an end of a length is, as to what its view draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Outline {
    /// On a line it draws: its extension line starts here — of the
    /// lines along that extension line (a face seen edge on), at the
    /// end nearest the dimension line (ISO 129-1: off the outline, not
    /// across the view).
    Drawn(Point2),
    /// Only on edges it hides and does not draw: the dimension points
    /// at nothing there.
    Hidden,
    /// On nothing drawn or hidden (a middle, a point of a sketch).
    Free,
}

impl Frame for Placed<'_> {
    fn place(&self, p: Point3) -> Point2 {
        Placed::place(self, p)
    }

    fn near_round(&self, c: Point2, r: f64, way: Point2) -> Option<Point2> {
        let mut best: Option<(f64, Point2)> = None;
        for (a, b) in drawn_lines(self) {
            let d = b - a;
            // (Its ends, and its point nearest the middle.)
            let t = ((c - a).dot(d) / d.length_squared().max(1e-300)).clamp(0.0, 1.0);
            for q in [a, b, a + d * t] {
                let off = (q.distance(c) - r).abs();
                if off > r * 0.5 {
                    continue;
                }
                let turn = 1.0 - (q - c).normalize_or_zero().dot(way);
                let cost = off + 0.2 * r * turn;
                if best.is_none_or(|(x, _)| cost < x) {
                    best = Some((cost, q));
                }
            }
        }
        best.map(|(_, q)| q)
    }

    fn way(&self, d: Point3) -> Point2 {
        Placed::way(self, d)
    }

    fn hidden_arc(&self, c: Point2, r: f64, way: Point2) -> Option<Point2> {
        let drawn = drawn_lines(self);
        let tol = (r * 0.01).max(0.05);
        let on = |p: Point2| (p.distance(c) - r).abs() <= tol;
        let pieces: Vec<(Point2, Point2)> = self
            .seen
            .lines
            .iter()
            .map(|l| (self.sheet(l.0), self.sheet(l.1)))
            .filter(|(a, b)| on(*a) && on(*b) && a.distance(*b) < r * 0.6)
            .filter(|piece| !drawn.contains(piece))
            .collect();
        let length: f64 = pieces.iter().map(|(a, b)| a.distance(*b)).sum();
        if length < r * 0.3 {
            return None;
        }
        let want = c + way * r;
        pieces
            .iter()
            .flat_map(|(a, b)| [*a, *b, (*a + *b) / 2.0])
            .map(|p| c + (p - c).normalize_or_zero() * r)
            .min_by(|p, q| p.distance(want).total_cmp(&q.distance(want)))
    }

    fn scale(&self) -> f64 {
        self.scale
    }

    fn towards(&self) -> Point3 {
        self.basis[2]
    }

    fn whole(&self) -> bool {
        self.reach.is_none()
    }

    fn circle(&self) -> Option<(Point2, f64)> {
        self.reach.map(|reach| (self.centre, reach * self.scale))
    }

    fn cut_off(&self, x: Point2) -> bool {
        if let Some(reach) = self.reach {
            return x.distance(self.centre) > reach * self.scale + 1e-6;
        }
        let Some((at, out, only)) = self.halved() else {
            return false;
        };
        if (x - at).dot(out) <= 1e-6 {
            return false;
        }
        // (In the half drawn uncut: off it where the model whole shows
        // nothing there, as a bore's wall is hidden.)
        let near = 0.05;
        only || !self.outside.as_ref().is_some_and(|o| {
            o.lines.iter().any(|(a, b, behind, _)| {
                let (a, b) = (self.sheet(*a), self.sheet(*b));
                let d = b - a;
                let t = ((x - a).dot(d) / d.length_squared().max(1e-300)).clamp(0.0, 1.0);
                !behind && (a + d * t).distance(x) <= near
            })
        })
    }

    fn shown_to(&self, a: Point2, toward: Point2) -> Option<f64> {
        if let Some(reach) = self.reach {
            let (c, r) = (self.centre, reach * self.scale);
            let f = a - c;
            let b = f.dot(toward);
            let disc = b * b - (f.dot(f) - r * r);
            let to = match disc >= 0.0 {
                true => -b + disc.sqrt(),
                false => (c - a).dot(toward) + r,
            };
            return Some(to.max(0.0));
        }
        let (at, out, _) = self.halved()?;
        let w = toward.dot(out);
        (w > 1e-9).then(|| ((at - a).dot(out) / w).max(0.0))
    }

    fn outline(&self, p: Point2, foot: Point2) -> Outline {
        Placed::outline(self, p, foot)
    }

    fn on_arc(&self, c: Point2, r: f64, way: Point2) -> Option<Point2> {
        let near = (r * 0.01).max(0.05);
        let want = c + way * r;
        // A whole circle seen square on.
        let whole = self.seen.rounds.iter().any(|(at, radius)| {
            self.sheet(*at).distance(c) <= near && (radius * self.scale - r).abs() <= near
        });
        // (Not one hidden, all or in part, behind what is nearer — a
        // tube behind its flange: of that, what is drawn.)
        if whole && self.hidden_arc(c, r, way).is_none() {
            return Some(want);
        }
        // Else the short pieces of drawn arcs about `c` as large (an
        // arc is drawn as short chords, each with ends on the circle —
        // or, cut short where it meets another line, within its sag of
        // it — and going on into the next): of them the point nearest
        // `want`, out on the circle.
        let exact = r * 1e-4 + 1e-3;
        let pieces: Vec<(Point2, Point2)> = drawn_lines(self)
            .into_iter()
            .filter(|(a, b)| {
                let length = a.distance(*b);
                let sag = r - (r * r - length * length / 4.0).max(0.0).sqrt();
                length > 1e-9
                    && length < r * 0.15
                    && [a, b].iter().all(|p| {
                        let off = p.distance(c) - r;
                        off <= exact && off >= -(sag + exact)
                    })
            })
            .collect();
        let joined = |k: usize| {
            let (a, b) = pieces[k];
            pieces.iter().enumerate().any(|(j, (p, q))| {
                j != k
                    && [p, q]
                        .iter()
                        .any(|x| x.distance(a) < exact || x.distance(b) < exact)
            })
        };
        let mut best: Option<(f64, Point2)> = None;
        for (k, (a, b)) in pieces.iter().copied().enumerate() {
            if !joined(k) {
                continue;
            }
            let d = b - a;
            let t = ((want - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
            let q = a + d * t;
            let q = c + (q - c).normalize_or_zero() * r;
            let off = q.distance(want);
            if best.is_none_or(|(o, _)| off < o) {
                best = Some((off, q));
            }
        }
        best.map(|(off, q)| if off < near { want } else { q })
    }

    fn drawn_along(&self, p: Point2, way: Point2) -> Vec<(f64, f64)> {
        let near = 0.05;
        let mut spans: Vec<(f64, f64)> = drawn_lines(self)
            .into_iter()
            .filter(|(a, b)| {
                (*a - p).perp_dot(way).abs() <= near && (*b - p).perp_dot(way).abs() <= near
            })
            .map(|(a, b)| {
                let (s, t) = ((a - p).dot(way), (b - p).dot(way));
                (s.min(t), s.max(t))
            })
            .collect();
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut joined: Vec<(f64, f64)> = Vec::new();
        for (s, t) in spans {
            match joined.last_mut() {
                Some(last) if s <= last.1 + near => last.1 = last.1.max(t),
                _ => joined.push((s, t)),
            }
        }
        joined
    }

    fn circles(&self) -> Vec<(Point2, f64)> {
        self.seen
            .rounds
            .iter()
            .map(|(at, r)| (self.sheet(*at), r * self.scale))
            .collect()
    }

    fn along(&self, p: Point2, way: Point2) -> Option<(f64, f64)> {
        let near = 0.05;
        let mut reach: Option<(f64, f64)> = None;
        for (a, b) in drawn_lines(self) {
            if (a - p).perp_dot(way).abs() > near || (b - p).perp_dot(way).abs() > near {
                continue;
            }
            let (s, t) = ((a - p).dot(way), (b - p).dot(way));
            reach = Some(match reach {
                Some((lo, hi)) => (lo.min(s.min(t)), hi.max(s.max(t))),
                None => (s.min(t), s.max(t)),
            });
        }
        reach
    }
}

/// What was drawn of a note: the box of its text (what is pointed at),
/// and the box of its text and its dimension line together (what the
/// next note keeps clear of).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drawn {
    pub(crate) text: [Point2; 2],
    pub(crate) body: [Point2; 2],
    /// Of a length: how it is drawn, to drag its parts by.
    pub(crate) linear: Option<super::LaidLinear>,
    /// Of a radius or a diameter seen square on: its line, from the
    /// round to the text (what another note's text keeps off).
    pub(crate) leader: Option<(Point2, Point2)>,
    /// Whether an end of it is on an edge the view hides and does not
    /// draw: it points at nothing there.
    pub(crate) unseen: bool,
    /// Whether an end of it is on nothing the view draws or hides: its
    /// extension line starts in empty space.
    pub(crate) loose: bool,
    /// Whether it is a half dimension: one end off what the view shows
    /// (outside a detail, in a half not drawn), its line running from
    /// the other a little past where the view ends.
    pub(crate) partial: bool,
}

impl Drawn {
    fn of(text: [Point2; 2]) -> Self {
        Self {
            text,
            body: text,
            linear: None,
            leader: None,
            unseen: false,
            loose: false,
            partial: false,
        }
    }

    fn led(text: [Point2; 2], leader: (Point2, Point2)) -> Self {
        Self {
            leader: Some(leader),
            ..Self::of(text)
        }
    }
}

/// A diameter drawn in a view — from the side as a length, or square on
/// across its circle: what a datum or a geometric tolerance of that
/// round feature's axis attaches to (ISO 1101, ISO 5459: on the
/// extension of its dimension line).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SizeLine {
    /// The round feature: a point of its axis, the axis, its radius.
    pub(crate) centre: Point3,
    pub(crate) axis: Point3,
    pub(crate) radius: f64,
    /// Its dimension line, from end to end: between the extension lines
    /// of a length; across the circle, through its middle, of one seen
    /// square on.
    pub(crate) line: [Point2; 2],
    /// The box of its text.
    pub(crate) text: [Point2; 2],
}

/// What a datum's or a geometric tolerance's leader may end on besides
/// its feature, in the view it is drawn in: the diameters drawn there
/// (of an axis, it ends on the extension of its dimension line), the
/// ends of the leaders of others drawn there already (two on one face
/// stand apart), and where the sheet says it is to end.
#[derive(Debug, Clone, Default)]
pub(crate) struct Attach {
    pub(crate) sizes: Vec<SizeLine>,
    pub(crate) taken: Vec<Point2>,
    pub(crate) to: Option<Point2>,
}

/// Where the leader of a datum or a geometric tolerance with its frame
/// (or square) about `at` ends in the view `placed`, and the way out
/// from there it leaves: of the axis of a round feature, on the
/// extension of its diameter's dimension line, else on the circle of it
/// — of what is alike, the nearest — where the view looks along it; of
/// a face (`on`, `out`) seen edge on, on the outline the view draws of
/// it, near the frame (never in a hole's gap), apart from the leaders
/// that end there already; of a face seen square on, on it.
fn feature_tip(
    placed: &dyn Frame,
    on: Point3,
    out: Point3,
    axis: Option<&workpiece_core::world::FeatureAxis>,
    rect: [Point2; 2],
    attach: &Attach,
) -> (Point2, Point2) {
    let at = (rect[0] + rect[1]) / 2.0;
    let wanted = attach.to.unwrap_or(at);
    if let Some(of) = axis {
        if let Some(found) = on_size_line(of, &attach.sizes, wanted) {
            return found;
        }
        if of.axis.dot(placed.towards()).abs() > 1.0 - 1e-6 {
            let r = of.radius * placed.scale();
            let own = placed.place(of.centre);
            let near = r * 1e-3 + 1e-6;
            let c = placed
                .circles()
                .into_iter()
                .filter(|(_, q)| (q - r).abs() <= near)
                .map(|(c, _)| c)
                .chain(std::iter::once(own))
                .min_by(|a, b| a.distance(wanted).total_cmp(&b.distance(wanted)))
                .unwrap_or(own);
            let way = (wanted - c)
                .try_normalize()
                .unwrap_or(Point2::new(1.0, 1.0).normalize());
            return (c + way * r, way);
        }
    }
    let p = placed.place(on);
    let way = placed.way(out);
    if way.length() <= 0.5 {
        // Seen square on: a dot on it.
        return (attach.to.unwrap_or(p), way);
    }
    let along = way.normalize().perp();
    let spans = placed.drawn_along(p, along);
    if spans.is_empty() {
        return (p, way);
    }
    // (A frame close in front of the outline — nearer than a leader
    // needs to come square off it — is reached from its end: the leader
    // ends a little past that end.)
    let out_way = way.normalize();
    let corners = [
        rect[0],
        rect[1],
        Point2::new(rect[0].x, rect[1].y),
        Point2::new(rect[1].x, rect[0].y),
    ];
    let off = corners
        .iter()
        .map(|c| (*c - p).dot(out_way))
        .fold(f64::INFINITY, f64::min);
    let (lo, hi) = corners
        .iter()
        .map(|c| (*c - p).dot(along))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| {
            (l.min(x), h.max(x))
        });
    let wanted = match attach.to {
        None if off > 0.0 && off < ARROW * 2.0 + lettering() => {
            let t = (wanted - p).dot(along);
            let ends = [lo - ARROW * 2.0, hi + ARROW * 2.0];
            let t = if (ends[0] - t).abs() <= (ends[1] - t).abs() {
                ends[0]
            } else {
                ends[1]
            };
            p + along * t
        }
        _ => wanted,
    };
    // (Within what is drawn, a little in from its ends where that is
    // long enough.)
    let inside = |t: f64| -> f64 {
        spans
            .iter()
            .map(|&(s, e)| {
                let margin = ((e - s) / 4.0).min(3.0);
                t.clamp(s + margin, e - margin)
            })
            .min_by(|x, y| (x - t).abs().total_cmp(&(y - t).abs()))
            .unwrap_or(t)
    };
    let t = inside((wanted - p).dot(along));
    // (Apart from the leaders that end on it already: on along it.)
    let free = |t: f64| {
        attach
            .taken
            .iter()
            .all(|q| q.distance(p + along * t) > ARROW * 1.5)
    };
    let t = (0..12_u32)
        .map(|k| {
            let step = f64::from(k.div_ceil(2)) * ARROW * 2.0;
            if k % 2 == 1 { t + step } else { t - step }
        })
        .map(inside)
        .find(|t| free(*t))
        .unwrap_or(t);
    (p + along * t, way)
}

/// Where the leader of a note of the axis `of` ends, of the frame (or
/// datum square) about `middle`: on an extension line of the diameter
/// of that feature drawn among `sizes`, in line with its dimension line
/// and from outside it (ISO 1101 fig. 6, ISO 5459) — the end its text
/// does not stand past, the nearer the frame. The tip and the way out
/// from it (along the dimension line).
fn on_size_line(
    of: &workpiece_core::world::FeatureAxis,
    sizes: &[SizeLine],
    middle: Point2,
) -> Option<(Point2, Point2)> {
    let size = sizes
        .iter()
        .find(|s| of.sized_by(s.centre, s.axis, s.radius))?;
    let [a, b] = size.line;
    let text = (size.text[0] + size.text[1]) / 2.0;
    (0..2)
        .filter_map(|k| {
            let (end, other) = if k == 0 { (a, b) } else { (b, a) };
            let out = (end - other).try_normalize()?;
            // (Its text put out past this end: the leader would run
            // into it.)
            let past = (text - end).dot(out) > 0.0;
            Some((past, middle.distance(end), end, out))
        })
        .min_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)))
        .map(|(_, _, end, out)| (end, out))
}

/// What stands on a sheet already, which what is put by itself keeps
/// clear of: the boxes of notes (their text and dimension line), the
/// lines of radii from their round to their text, and the views that
/// are not the note's own.
#[derive(Debug, Clone, Default)]
pub(crate) struct Taken {
    pub(crate) boxes: Vec<[Point2; 2]>,
    pub(crate) leaders: Vec<(Point2, Point2)>,
    pub(crate) views: Vec<[Point2; 2]>,
    /// The dimension lines of lengths: another along the same line
    /// over the same stretch would be drawn over them.
    pub(crate) lines: Vec<[Point2; 2]>,
    /// Where on the sheet lettering may stand: within its frame, clear
    /// of the edge (what stands out past it is brought in, onto what
    /// is there).
    pub(crate) within: Option<[Point2; 2]>,
}

/// Whether the dimension lines `a` and `b` run along one line (within
/// 2 mm of it) over a common stretch: the one would be drawn over the
/// other (a chain, end to end, is not).
fn on_one_line(a: &[Point2; 2], b: &[Point2; 2]) -> bool {
    let Some(way) = (a[1] - a[0]).try_normalize() else {
        return false;
    };
    let Some(other) = (b[1] - b[0]).try_normalize() else {
        return false;
    };
    if way.perp_dot(other).abs() > 1e-3 || (b[0] - a[0]).dot(way.perp()).abs() > 2.0 {
        return false;
    }
    let span = |p: &[Point2; 2]| {
        let (s, t) = (p[0].dot(way), p[1].dot(way));
        (s.min(t), s.max(t))
    };
    let ((a0, a1), (b0, b1)) = (span(a), span(b));
    a1.min(b1) - a0.max(b0) > 0.5
}

impl Taken {
    pub(crate) fn push(&mut self, drawn: &Drawn) {
        self.boxes.push(drawn.body);
        self.leaders.extend(drawn.leader);
        self.lines.extend(drawn.linear.map(|l| l.line));
    }

    /// Whether `drawn` is in the way of what is taken: its box near a
    /// box, a leader through its box, or its own leader through a box.
    fn blocks(&self, drawn: &Drawn) -> bool {
        let grown = [
            drawn.body[0] - Point2::splat(1.0),
            drawn.body[1] + Point2::splat(1.0),
        ];
        self.boxes.iter().any(|other| near(&drawn.body, other, 1.5))
            || drawn
                .linear
                .is_some_and(|l| self.lines.iter().any(|other| on_one_line(&l.line, other)))
            || self.leaders.iter().any(|(a, b)| crosses(*a, *b, &grown))
            || drawn
                .leader
                .is_some_and(|(a, b)| self.boxes.iter().any(|other| crosses(a, b, other)))
    }

    /// Whether `drawn` reaches onto a view that is not its own, or
    /// its text off the sheet's frame.
    fn on_a_view(&self, drawn: &Drawn) -> bool {
        self.views.iter().any(|v| near(&drawn.body, v, 1.0))
            || self.within.is_some_and(|[lo, hi]| {
                drawn.text[0].cmplt(lo).any() || drawn.text[1].cmpgt(hi).any()
            })
    }
}

/// The box a text takes on the sheet.
pub(crate) fn text_corners(text: &super::Text, shrink: f64) -> [Point2; 4] {
    let along = from_angle(text.angle);
    let up = along.perp();
    let (w, h) = (width(&text.text, text.height), text.height);
    let foot = text.at - along * (w * text.anchor[0]) - up * (h * text.anchor[1]);
    let (lo, w, h) = (
        foot + (along + up) * shrink,
        (w - 2.0 * shrink).max(0.0),
        (h - 2.0 * shrink).max(0.0),
    );
    [lo, lo + along * w, lo + along * w + up * h, lo + up * h]
}

/// Whether the four-cornered shapes `a` and `b` (each convex, its
/// corners in turn) overlap: no side of either parts them.
pub(crate) fn overlap(a: &[Point2; 4], b: &[Point2; 4]) -> bool {
    for shape in [a, b] {
        for k in 0..4 {
            let normal = (shape[(k + 1) % 4] - shape[k]).perp();
            if normal == Point2::ZERO {
                continue;
            }
            let span = |c: &[Point2; 4]| {
                c.iter()
                    .map(|p| p.dot(normal))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                        (lo.min(x), hi.max(x))
                    })
            };
            let ((alo, ahi), (blo, bhi)) = (span(a), span(b));
            if ahi <= blo || bhi <= alo {
                return false;
            }
        }
    }
    true
}

pub(crate) fn text_box(text: &super::Text) -> [Point2; 2] {
    let along = from_angle(text.angle);
    let up = along.perp();
    let (w, h) = (width(&text.text, text.height), text.height);
    let foot = text.at - along * (w * text.anchor[0]) - up * (h * text.anchor[1]);
    let corners = [
        foot,
        foot + along * w,
        foot + up * h,
        foot + along * w + up * h,
    ];
    let lo = corners
        .iter()
        .fold(Point2::splat(f64::INFINITY), |m, p| m.min(*p));
    let hi = corners
        .iter()
        .fold(Point2::splat(f64::NEG_INFINITY), |m, p| m.max(*p));
    [lo, hi]
}

/// Whether the line from `a` to `b` runs through the box `rect`.
fn crosses(a: Point2, b: Point2, rect: &[Point2; 2]) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-d.x, a.x - rect[0].x),
        (d.x, rect[1].x - a.x),
        (-d.y, a.y - rect[0].y),
        (d.y, rect[1].y - a.y),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    t0 < t1
}

/// How far in from the sheet's edge lettering keeps, in millimetres,
/// at the left and elsewhere: within the frame (ISO 5457 draws it 20 in
/// at the left, 10 elsewhere).
const FRAMED: [f64; 2] = [21.0, 11.0];

/// How long an extension line or a leader may be, in millimetres on
/// the sheet, before the build says its text stands too far off.
const LONGEST: f64 = 80.0;

/// The length of an arrowhead, how far an extension line runs on past
/// its dimension line, and how far the text stands off the line.
const ARROW: f64 = 3.5;
/// How nearly square on (the cosine of the angle off it) a round must
/// be seen to have its radius shown on its arc: within about 30°.
const SLANT: f64 = 0.85;
const PAST: f64 = 2.0;
const OFF: f64 = 1.0;

/// How wide `text` is, its capitals `height` high: as the font it is
/// set in (Helvetica, its capitals 0.718 of its size) has its letters
/// — what the next piece of a line of several is set on by, so that
/// none is written over the one before (`Ø` is wider than a digit).
pub(crate) fn width(text: &str, height: f64) -> f64 {
    let size = height / 0.718;
    text.chars().map(super::pdf::advance).sum::<f64>() / 1000.0 * size
}

/// Whether the view `placed` sees `note` true: a length square to how
/// it looks, something round square on or from the side, an angle flat.
pub(crate) fn fits(note: &Annotation, placed: &dyn Frame) -> bool {
    placed.whole() && shows(note, placed, false)
}

/// Whether the view `placed` — a detail too, where a note stands only
/// when it is put there — sees `note` true; or, for a length along one
/// of the model's axes, at least along its length (a view from a
/// corner: an isometric one — where a dimension stands only when it is
/// put there).
pub(crate) fn sees(note: &Annotation, placed: &dyn Frame) -> bool {
    shows(note, placed, true)
}

fn shows(note: &Annotation, placed: &dyn Frame, from_a_corner: bool) -> bool {
    match &note.note {
        Note::Dimension {
            measured,
            elsewhere,
            ..
        } => std::iter::once(measured)
            .chain(elsewhere)
            .any(|m| shows_measured(m, placed, from_a_corner)),
        // (Best where the face is seen edge on; but it can be pointed
        // at anywhere.)
        Note::Datum { .. } | Note::Geometric { .. } | Note::Texture { .. } => true,
        // A hole: seen as a circle, or from the side.
        Note::Hole { axis, .. } => {
            let facing = axis.dot(placed.towards()).abs();
            !(1e-9..=1.0 - 1e-9).contains(&facing)
        }
    }
}

/// `note` as the view `placed` shows it: a dimension measured at the
/// first of its places that the view sees true (seen true as `fits`
/// has it before as `sees` does); nothing if the view sees it nowhere.
/// Of the places the view sees it true, the one nearest `near` (where
/// its text stands, on the sheet), if that is given: a fillet's radius
/// at the round its text stands by, not at another of its corners.
pub(crate) fn as_seen<'a>(
    note: &'a Annotation,
    placed: &dyn Frame,
    near: Option<Point2>,
) -> Option<std::borrow::Cow<'a, Annotation>> {
    use std::borrow::Cow;
    let Note::Dimension {
        measured,
        value,
        tolerance,
        elsewhere,
    } = &note.note
    else {
        return sees(note, placed).then_some(Cow::Borrowed(note));
    };
    let places = || std::iter::once(measured).chain(elsewhere);
    let off = |m: &Measured| {
        near.map_or(0.0, |at| {
            // (Of something round, how far off its round the text is:
            // the round it is put by, of several alike.)
            if let Measured::Radius { centre, radius, .. }
            | Measured::Diameter { centre, radius, .. } = m
            {
                return (placed.place(*centre).distance(at) - radius * placed.scale()).abs();
            }
            let anchors = match m {
                Measured::Length { from, to, .. } => vec![*from, *to],
                Measured::Diameter { centre, .. }
                | Measured::Radius { centre, .. }
                | Measured::Radial { centre, .. } => vec![*centre],
                Measured::Angle { at, .. } => vec![*at],
            };
            let middle =
                anchors.iter().map(|p| placed.place(*p)).sum::<Point2>() / anchors.len() as f64;
            middle.distance(at)
        })
    };
    let nearest = |true_only: bool| {
        places()
            .filter(|m| shows_measured(m, placed, !true_only))
            .min_by(|a, b| off(a).total_cmp(&off(b)))
    };
    let pick = nearest(true).or_else(|| nearest(false))?;
    if std::ptr::eq(pick, measured) {
        return Some(Cow::Borrowed(note));
    }
    Some(Cow::Owned(Annotation {
        note: Note::Dimension {
            measured: pick.clone(),
            value: *value,
            tolerance: tolerance.clone(),
            elsewhere: Vec::new(),
        },
        ..note.clone()
    }))
}

/// Whether the view `placed` sees what `measured` measures true (see
/// [`sees`] for `from_a_corner`).
fn shows_measured(measured: &Measured, placed: &dyn Frame, from_a_corner: bool) -> bool {
    // (Of what a detail shows only what is in it: a length with an end
    // in its circle (across the way it measures); something
    // round that does; an angle whose apex is in it, or whose lines
    // both cross it.)
    let in_it = match placed.circle() {
        None => true,
        Some((c, r)) => match measured {
            Measured::Length { from, to, along } => {
                let (p, q) = (placed.place(*from), placed.place(*to));
                let way = along
                    .map(|a| placed.way(a))
                    .unwrap_or(q - p)
                    .try_normalize()
                    .unwrap_or(Point2::X);
                // (At least one end in it: the other, if it is not,
                // is drawn as a half dimension.)
                let inside = |x: Point2| (x - c).dot(way).abs() <= r;
                inside(p) || inside(q)
            }
            Measured::Diameter { centre, radius, .. }
            | Measured::Radius { centre, radius, .. }
            | Measured::Radial {
                centre,
                outer: radius,
                ..
            } => placed.place(*centre).distance(c) <= r + radius * placed.scale(),
            // (Or whose two lines both cross it: their apex off it, the
            // arc where they are.)
            Measured::Angle { at, first, second } => {
                let v = placed.place(*at);
                let crosses = |d: workpiece_core::math::Point3| {
                    placed
                        .way(d)
                        .try_normalize()
                        .is_some_and(|w| (c - v).perp_dot(w).abs() < r)
                };
                v.distance(c) <= r || (crosses(*first) && crosses(*second))
            }
        },
    };
    if !in_it {
        return false;
    }
    let towards = placed.towards();
    let flat =
        |d: workpiece_core::math::Point3| d.dot(towards).abs() < 1e-6 * d.length().max(1e-300);
    // Along one of the model's axes, and not seen end on.
    let along_an_axis = |d: workpiece_core::math::Point3| {
        let Some(d) = d.try_normalize() else {
            return false;
        };
        let parts = d.abs().to_array();
        from_a_corner
            && parts.iter().filter(|x| **x < 1e-9).count() == 2
            && d.dot(towards).abs() < 0.95
    };
    // Round about `axis`, seen square on or from the side; a ball
    // (which has no axis) from anywhere.
    let round = |axis: workpiece_core::math::Point3| {
        axis == workpiece_core::math::Point3::ZERO
            || flat(axis)
            || axis.dot(towards).abs() > 1.0 - 1e-9
    };
    match measured {
        Measured::Length { from, to, along } => match along {
            Some(along) => flat(*along) || along_an_axis(*along),
            None => {
                (*to - *from).length() > 0.0 && (flat(*to - *from) || along_an_axis(*to - *from))
            }
        },
        Measured::Diameter { axis, .. } | Measured::Radial { axis, .. } => round(*axis),
        // (A radius goes by itself where its arc shows; from the
        // side, as a length out from the axis, where it is put.)
        // (Or seen slantwise, as an arc still: said as its true size.)
        Measured::Radius { axis, .. } => {
            square_on(*axis, towards)
                || (from_a_corner && flat(*axis))
                || axis.dot(towards).abs() >= SLANT
        }
        Measured::Angle { first, second, .. } => flat(*first) && flat(*second),
    }
}

/// The ends of a length from `from` to `to` (along `along`, if not
/// straight from the one to the other) on the sheet, and the way it
/// measures there (a unit vector). In a detail, both ends slid square
/// to that way to across the detail's middle: what it measures is in
/// it there (it is shown only where it reaches into the circle), not
/// wherever on the faces the length was taken.
fn length_ends(
    placed: &dyn Frame,
    from: Point3,
    to: Point3,
    along: Option<Point3>,
) -> Option<((Point2, Point2), Point2)> {
    let (p, q) = (placed.place(from), placed.place(to));
    let along = match along {
        Some(along) => placed.way(along),
        None => q - p,
    }
    .try_normalize()?;
    let Some((c, _)) = placed.circle() else {
        return Some(((p, q), along));
    };
    let across = along.perp();
    let slide = |x: Point2| x + across * (c - x).dot(across);
    Some(((slide(p), slide(q)), along))
}

/// A text along the direction `along`, standing off the point `at` on
/// the side `along` turned left is; turned to be read from below or
/// from the right. Gives its box on the sheet.
fn text_along(laid: &mut Laid, at: Point2, along: Point2, text: &str, height: f64) -> [Point2; 2] {
    let mut along = along.normalize_or_zero();
    if along == Point2::ZERO {
        along = Point2::X;
    }
    // (Read from below, or from the right.)
    if along.x < -1e-9 || (along.x.abs() <= 1e-9 && along.y < 0.0) {
        along = -along;
    }
    let up = along.perp();
    let foot = at + up * OFF;
    laid.texts.push(super::Text {
        at: foot,
        text: text.to_owned(),
        height,
        anchor: [0.5, 0.0],
        angle: libm::atan2(along.y, along.x),
        preview: false,
    });
    let half = along * (width(text, height) / 2.0);
    let corners = [
        foot - half,
        foot + half,
        foot - half + up * height,
        foot + half + up * height,
    ];
    let lo = corners
        .iter()
        .fold(Point2::splat(f64::INFINITY), |m, p| m.min(*p));
    let hi = corners
        .iter()
        .fold(Point2::splat(f64::NEG_INFINITY), |m, p| m.max(*p));
    [lo, hi]
}

/// What a dimension says: its value, with what it is of before it; and
/// its deviations, to be set small after it, the upper over the lower
/// (ISO 129-1: as many decimals as each other, a zero bare; the unit of
/// an angle after each).
fn said(value: f64, prefix: &str, unit: &str, tolerance: &Tolerance) -> Said {
    use workpiece_core::world::annotation::{alike, deviations};
    let base = format!("{prefix}{}{unit}", number(value));
    let plain = |text: String| Said {
        text,
        over: None,
        after: String::new(),
        high: lettering(),
        boxed: false,
    };
    match tolerance {
        Tolerance::Deviations { upper, lower } => Said {
            text: base,
            over: Some(deviations(*upper, *lower, unit)),
            after: String::new(),
            high: lettering() * 0.7,
            boxed: false,
        },
        // The two limits of size, the upper over the lower, as high as
        // any value.
        Tolerance::Limits { upper, lower } => {
            let [u, l] = alike(value + upper, value + lower);
            Said {
                text: prefix.to_owned(),
                over: Some([format!("{u}{unit}"), format!("{l}{unit}")]),
                after: String::new(),
                high: lettering(),
                boxed: false,
            }
        }
        // For reference: in brackets.
        Tolerance::Reference => plain(format!("({base})")),
        // Theoretically exact (ISO 1101): in a box.
        Tolerance::Exact => Said {
            boxed: true,
            ..plain(base)
        },
        // A tolerance class after the size; with its deviations, where
        // the sheet says, small in brackets after it.
        Tolerance::Fit(class) => match tolerance.fit_deviations(value) {
            Some(over) => Said {
                text: format!("{base} {class} ("),
                over: Some(over),
                after: ")".to_owned(),
                high: lettering() * 0.7,
                boxed: false,
            },
            None => plain(format!("{base} {class}")),
        },
        other => plain(format!("{base}{}", other.suffix(value, unit))),
    }
}

/// What a dimension says: its text, and what is set after it one over
/// the other (deviations, or limits of size) and how high, and what
/// closes it after that (a bracket).
struct Said {
    text: String,
    over: Option<[String; 2]>,
    after: String,
    high: f64,
    /// Whether it is written in a box: a theoretically exact size.
    boxed: bool,
}

impl Said {
    /// How much room it takes along its line.
    fn room(&self) -> f64 {
        let over = self.over.as_ref().map_or(0.0, |o| {
            width(&o[0], self.high).max(width(&o[1], self.high))
        });
        width(&self.text, lettering())
            + over
            + width(&self.after, lettering())
            + 2.0
            + if self.boxed { 1.6 } else { 0.0 }
    }
}

/// A dimension's text on its line at `at` along `along`, with its
/// deviations if it has them. Gives its box.
fn dimension_text(laid: &mut Laid, at: Point2, along: Point2, said: &Said) -> [Point2; 2] {
    let Some([upper, lower]) = &said.over else {
        let rect = text_along(laid, at, along, &said.text, lettering());
        if said.boxed {
            // (The box about the text, along its line, a little clear
            // of it all round: what is written is the box's.)
            let mut along = along.normalize_or(Point2::X);
            if along.x < -1e-9 || (along.x.abs() <= 1e-9 && along.y < 0.0) {
                along = -along;
            }
            let up = along.perp();
            let half = width(&said.text, lettering()) / 2.0 + 0.8;
            let (foot, top) = (at + up * (OFF - 0.8), at + up * (OFF + lettering() + 0.8));
            let corners = [
                foot - along * half,
                foot + along * half,
                top + along * half,
                top - along * half,
            ];
            for k in 0..4 {
                laid.line(corners[k], corners[(k + 1) % 4], Pen::Narrow);
            }
            let lo = corners.iter().fold(rect[0], |m, p| m.min(*p));
            let hi = corners.iter().fold(rect[1], |m, p| m.max(*p));
            return [lo, hi];
        }
        return rect;
    };
    // The value, and after it the deviations, 0.7 as high, one over
    // the other.
    let mut along = along.normalize_or_zero();
    if along == Point2::ZERO {
        along = Point2::X;
    }
    if along.x < -1e-9 || (along.x.abs() <= 1e-9 && along.y < 0.0) {
        along = -along;
    }
    let small = said.high;
    let (w, d, a) = (
        width(&said.text, lettering()),
        width(upper, small).max(width(lower, small)),
        width(&said.after, lettering()),
    );
    let start = at - along * ((w + d + a) / 2.0);
    let up = along.perp();
    let mut rect = if said.text.is_empty() {
        let foot = start + up * OFF;
        [foot, foot]
    } else {
        text_along(
            laid,
            start + along * (w / 2.0),
            along,
            &said.text,
            lettering(),
        )
    };
    // (The lower stands on the line the value stands on: nothing of
    // it reaches down to the dimension line.)
    for (text, lift) in [(lower, OFF), (upper, OFF + small * 1.35)] {
        laid.texts.push(super::Text {
            at: start + along * (w + 0.4) + up * lift,
            text: text.clone(),
            height: small,
            anchor: [0.0, 0.0],
            angle: libm::atan2(along.y, along.x),
            preview: false,
        });
    }
    if !said.after.is_empty() {
        let after = text_along(
            laid,
            start + along * (w + 0.8 + d + a / 2.0),
            along,
            &said.after,
            lettering(),
        );
        rect = [rect[0].min(after[0]), rect[1].max(after[1])];
    }
    let d = d + a + 0.8 * f64::from(u8::from(a > 0.0));
    for corner in [
        start + up * OFF,
        start + along * (w + d) + up * OFF,
        start + up * (OFF + small * 2.35),
        start + along * (w + d) + up * (OFF + small * 2.35),
    ] {
        rect = [rect[0].min(corner), rect[1].max(corner)];
    }
    rect
}

/// A length from `p` to `q` on the sheet measured along `along` (a unit
/// vector), its dimension line through `at`: extension lines, the
/// dimension line with its arrows, and its text. Gives the text's box.
fn linear(
    laid: &mut Laid,
    (p, q): (Point2, Point2),
    along: Point2,
    at: Point2,
    said: &Said,
    starts: [f64; 2],
    half: Option<(usize, f64)>,
) -> Drawn {
    let across = along.perp();
    let (a, b) = (
        p + across * (at - p).dot(across),
        q + across * (at - q).dot(across),
    );
    if let Some((off, run)) = half {
        return half_linear(laid, [p, q], [a, b], along, at, said, starts, (off, run));
    }
    // Each extension line from where it starts, off what it measures,
    // on a little past the dimension line.
    let mut ways = [Point2::ZERO; 2];
    for (k, (from, to)) in [(p, a), (q, b)].into_iter().enumerate() {
        let way = (to - from).normalize_or_zero();
        ways[k] = way;
        let start = starts[k].min(from.distance(to));
        if way != Point2::ZERO {
            laid.line(from + way * start, to + way * PAST, Pen::Narrow);
        }
    }
    let (sa, sb, st) = (a.dot(along), b.dot(along), at.dot(along));
    let (lo, hi) = (sa.min(sb), sa.max(sb));
    // A text with no room between the extension lines stands outside
    // them, past the end on the side it is put (`at`): past the second
    // where it is put in the middle.
    let room = said.room();
    let st = if hi - lo < room && st > lo - room / 2.0 && st < hi + room / 2.0 {
        if st < (lo + hi) / 2.0 - 1e-9 {
            lo - 2.0 * ARROW - room / 2.0
        } else {
            hi + 2.0 * ARROW + room / 2.0
        }
    } else {
        st
    };
    let inside = hi - lo >= 2.0 * ARROW + 2.0;
    // (Nor on its own arrows (ISO 129-1): where it would stand on one,
    // between them if there is room, else past the nearer one.)
    let st = {
        let arrows = if inside {
            [(lo, lo + ARROW), (hi - ARROW, hi)]
        } else {
            [(lo - ARROW, lo), (hi, hi + ARROW)]
        };
        let on_arrow = |s: f64| {
            arrows
                .iter()
                .any(|(f, t)| s - room / 2.0 < *t && s + room / 2.0 > *f)
        };
        if on_arrow(st) {
            let between = if inside {
                hi - lo - 2.0 * ARROW
            } else {
                hi - lo
            };
            let (before, after) = (
                arrows[0].0 - room / 2.0 - 0.5,
                arrows[1].1 + room / 2.0 + 0.5,
            );
            // (Moved off the arrow as little as it takes: in from it
            // where there is room between them — not to the middle,
            // where a diameter's text would stand on the axis — else
            // past it.)
            let (first, last) = (
                arrows[0].1 + room / 2.0 + 0.5,
                arrows[1].0 - room / 2.0 - 0.5,
            );
            // (Between the arrows where it is put between them and fits;
            // past one only where it is put past it, or does not fit.)
            let fits = inside && between >= room + 1.0;
            let mut places = if fits && st >= lo && st <= hi {
                vec![]
            } else {
                // (Of two as near, past the second end, as before.)
                vec![after, before]
            };
            if fits {
                places.extend([first, last]);
            } else if between >= room && !on_arrow((lo + hi) / 2.0) {
                places.push((lo + hi) / 2.0);
            }
            places
                .into_iter()
                .min_by(|x, y| (x - st).abs().total_cmp(&(y - st).abs()))
                .unwrap_or(st)
        } else {
            st
        }
    };
    let on_line = |s: f64| a + along * (s - sa);
    // The line from arrow to arrow, and on to the text if that stands
    // outside; arrows from inside, or (too close) from outside.
    let (from, to) = (lo.min(st), hi.max(st));
    let (from, to) = if inside {
        (from, to)
    } else {
        (from.min(lo - 2.0 * ARROW), to.max(hi + 2.0 * ARROW))
    };
    laid.line(on_line(from), on_line(to), Pen::Narrow);
    let sign = if inside { 1.0 } else { -1.0 };
    laid.arrow(on_line(lo), -along * sign, ARROW);
    laid.arrow(on_line(hi), along * sign, ARROW);
    let text = dimension_text(laid, on_line(st), along, said);
    let ends = [on_line(from), on_line(to)];
    Drawn {
        text,
        body: [
            text[0].min(ends[0]).min(ends[1]),
            text[1].max(ends[0]).max(ends[1]),
        ],
        linear: Some(super::LaidLinear {
            from: [p, q],
            ways,
            line: [a, b],
            room: said.room(),
        }),
        leader: None,
        unseen: false,
        loose: false,
        partial: false,
    }
}

/// How far past where the view ends the line of a half dimension runs.
const HALF_PAST: f64 = 3.0;

/// Whether the length from `p` to `q` (on the sheet, along `along`) in
/// `placed`, its line through `at`, is a half dimension (ISO 129-1):
/// one end off what the view shows. Which end, and how far its line
/// runs from the other end's foot towards it — a little past where the
/// view ends.
fn halved(
    placed: &dyn Frame,
    (p, q): (Point2, Point2),
    along: Point2,
    at: Point2,
) -> Option<(usize, f64)> {
    let off = match (placed.cut_off(p), placed.cut_off(q)) {
        (true, false) => 0,
        (false, true) => 1,
        _ => return None,
    };
    let (inner, outer) = if off == 0 { (q, p) } else { (p, q) };
    let across = along.perp();
    let foot = inner + across * (at - inner).dot(across);
    let toward = (outer - inner).dot(along).signum() * along;
    let shown = placed.shown_to(foot, toward)?;
    // (Never as far as the other end, nor shorter than its arrow.)
    let whole = (outer - inner).dot(along).abs();
    let run = shown + HALF_PAST.min((whole - shown) / 2.0);
    (shown < whole).then_some((off, run.max(2.0 * ARROW)))
}

/// A half dimension (ISO 129-1): of the length from `from[0]` to
/// `from[1]` with its feet `feet` on its dimension line, the end `off`
/// is not shown — one extension line, one arrowhead at the other's
/// foot, the line running `run` from there towards the end not shown;
/// the text says the whole length.
#[allow(clippy::too_many_arguments)]
fn half_linear(
    laid: &mut Laid,
    from: [Point2; 2],
    feet: [Point2; 2],
    along: Point2,
    at: Point2,
    said: &Said,
    starts: [f64; 2],
    (off, run): (usize, f64),
) -> Drawn {
    let inner = 1 - off;
    let (x, foot) = (from[inner], feet[inner]);
    let mut ways = [Point2::ZERO; 2];
    let way = (foot - x).normalize_or_zero();
    ways[inner] = way;
    if way != Point2::ZERO {
        let start = starts[inner].min(x.distance(foot));
        laid.line(x + way * start, foot + way * PAST, Pen::Narrow);
    }
    // Along the line: from the foot (`s` 0) towards the end not shown.
    let toward = (feet[off] - foot).dot(along).signum() * along;
    let toward = if toward == Point2::ZERO {
        along
    } else {
        toward
    };
    let room = said.room();
    let mut st = (at - foot).dot(toward);
    // (Not on its arrow, nor behind it.)
    if st < ARROW + room / 2.0 + 0.5 {
        st = ARROW + room / 2.0 + 0.5;
    }
    let to = run.max(st + room / 2.0 + 1.0);
    let on_line = |s: f64| foot + toward * s;
    laid.line(foot, on_line(to), Pen::Narrow);
    laid.arrow(foot, -toward, ARROW);
    let text = dimension_text(laid, on_line(st), along, said);
    let ends = [foot, on_line(to)];
    Drawn {
        text,
        body: [
            text[0].min(ends[0]).min(ends[1]),
            text[1].max(ends[0]).max(ends[1]),
        ],
        linear: Some(super::LaidLinear {
            from,
            ways,
            line: feet,
            room,
        }),
        leader: None,
        unseen: false,
        loose: false,
        partial: true,
    }
}

/// A radius (or, with `diameter`, a diameter) of something round about
/// `c` (on the sheet), `r` millimetres large, seen square on, with its
/// text towards `at` (ISO 129-1). The arrowhead stands on the arc as
/// the view draws it — of an arc, at its point nearest the way to the
/// text; the line runs through the arrow towards the centre. `at` says
/// only which way from the centre and how far along that line the text
/// stands. Outside the round: a short leader from the text to the
/// arrow on the arc, pointing in. Inside: a radius from (near) the
/// centre out to the arc; a diameter of a whole circle across it, an
/// arrow at each side. A round the view does not draw (a pitch circle)
/// is drawn, as a centre line, so that what is said of it stands on it.
#[allow(clippy::too_many_arguments)]
fn round_dimension(
    laid: &mut Laid,
    placed: &dyn Frame,
    c: Point2,
    r: f64,
    at: Point2,
    said: &Said,
    diameter: bool,
    given: Option<Point2>,
) -> Drawn {
    let wanted = (at - c)
        .try_normalize()
        .unwrap_or(Point2::new(1.0, 1.0).normalize());
    // (Where the arc is given — seen slantwise, an ellipse — the arrow
    // stands there, and the radius is as far as that on the sheet.)
    let (mut way, drawn, r) = match given {
        Some(p) => (
            (p - c).try_normalize().unwrap_or(wanted),
            true,
            p.distance(c),
        ),
        None => match placed.on_arc(c, r, wanted) {
            // (A radius of an arc that does not reach round to the side
            // of the text — a hollow round, the text off the part —
            // where it reaches round to the other: from the text
            // through the centre out to it, one straight line.)
            Some(p)
                if !diameter
                    && (p - c).normalize_or_zero().dot(wanted) < 0.87
                    && placed
                        .on_arc(c, r, -wanted)
                        .is_some_and(|q| (q - c).normalize_or_zero().dot(-wanted) > 0.995) =>
            {
                (-wanted, true, r)
            }
            Some(p) => ((p - c).try_normalize().unwrap_or(wanted), true, r),
            None => (wanted, false, r),
        },
    };
    // (A radius of a round the view does not draw as such — a round cut
    // slantwise by a section, a blend that is no circle there: its
    // arrow on the outline the view draws where the round is, out from
    // its middle, nothing drawn of the circle.)
    // (A round the view hides and does not draw: its arrow on what it
    // hides of it, nearest the text's way, the arc drawn there as a
    // hidden line.)
    let hidden = !drawn && !diameter && given.is_none() && {
        match placed.hidden_arc(c, r, wanted) {
            Some(p) => {
                way = (p - c).try_normalize().unwrap_or(way);
                true
            }
            None => false,
        }
    };
    if let (false, false, None, false) = (drawn, diameter, given, hidden)
        && let Some(tip) = placed.near_round(c, r, way)
    {
        // (Its text on a leader to that point, level on a shoulder.)
        let knee = if at.distance(tip) > 2.0 * ARROW {
            at
        } else {
            tip + (tip - c).normalize_or(Point2::X) * (2.0 * ARROW)
        };
        laid.arrow(tip, tip - knee, ARROW);
        laid.line(tip, knee, Pen::Narrow);
        if ALIGNED.with(std::cell::Cell::get) {
            let text = dimension_text(laid, knee, knee - tip, said);
            return Drawn::led(text, (tip, knee));
        }
        let side = if knee.x >= tip.x { 1.0 } else { -1.0 };
        let end = knee + Point2::new(side * said.room(), 0.0);
        laid.line(knee, end, Pen::Narrow);
        let text = dimension_text(laid, (knee + end) / 2.0, Point2::X, said);
        return Drawn::led(text, (tip, knee));
    }
    if !drawn {
        // (Of a radius, as much of the arc as the arrow needs: a centre
        // line, or where the view hides the round, a hidden line.)
        let pen = if hidden { Pen::Dashed } else { Pen::Chain };
        match diameter {
            true => laid.circle(c, r, pen),
            false => {
                // (Of a hidden one only about the arrow: the rest of
                // that circle may be no edge at all.)
                let reach = if hidden { 2.0 * ARROW } else { 12.0 };
                let half = (reach / r.max(1e-9)).min(std::f64::consts::PI);
                laid.arc(c, r, libm::atan2(way.y, way.x) - half, 2.0 * half, pen);
            }
        }
    }
    // (Whether the round goes on round to the other side.)
    let across = !drawn
        || given.is_none()
            && placed
                .on_arc(c, r, -way)
                .is_some_and(|q| q.distance(c - way * r) <= (r * 0.01).max(0.05));
    let s = (at - c).dot(way);
    let half = said.room() / 2.0;
    let tip = c + way * r;
    // Outside, on a leader: its text level on a short shoulder at the
    // leader's end (ISO 129-1), as drawings mostly have it — unless the
    // sheet writes it along the leader.
    if s > r && !ALIGNED.with(std::cell::Cell::get) {
        laid.arrow(tip, -way, ARROW);
        // (The leader out along the radius as far as the text is high,
        // where it is put off that line — an arc that does not reach
        // round to the side of the text — the shoulder level from
        // there under it: the text where it is put.)
        let along = if way.y.abs() > 0.2 {
            (at.y - c.y) / way.y
        } else {
            at.distance(c)
        };
        let knee = c + way * along.max(r + 2.0 * ARROW);
        laid.line(tip, knee, Pen::Narrow);
        // (The shoulder towards the side the leader points to, on to
        // under the text.)
        let off = way.y.abs() > 0.2 && (at.x - knee.x).abs() > said.room() / 2.0;
        let side = match (off, way.x >= -1e-9) {
            (true, _) => (at.x - knee.x).signum(),
            (false, true) => 1.0,
            (false, false) => -1.0,
        };
        let long = if off {
            (at.x - knee.x).abs() + said.room() / 2.0
        } else {
            said.room()
        };
        let end = knee + Point2::new(side * long, 0.0);
        laid.line(knee, end, Pen::Narrow);
        let text = dimension_text(
            laid,
            end - Point2::new(side * said.room() / 2.0, 0.0),
            Point2::X,
            said,
        );
        return Drawn::led(text, (tip, knee));
    }
    // Past the centre (the arrow on the far side of it, a hollow
    // round): the leader from the arrow straight through the centre on
    // to the text, level on a shoulder there.
    if s < 0.0 && !diameter && !ALIGNED.with(std::cell::Cell::get) {
        laid.arrow(tip, way, ARROW);
        let knee = c + way * s.min(-2.0 * ARROW);
        laid.line(tip, knee, Pen::Narrow);
        let side = if way.x <= 1e-9 { 1.0 } else { -1.0 };
        let end = knee + Point2::new(side * said.room(), 0.0);
        laid.line(knee, end, Pen::Narrow);
        let text = dimension_text(laid, (knee + end) / 2.0, Point2::X, said);
        return Drawn::led(text, (tip, knee));
    }
    let (from, to) = if s > r {
        // Outside: the leader from the arrow out under the text.
        laid.arrow(tip, -way, ARROW);
        (tip, c + way * (s + half).max(r + 2.0 * ARROW))
    } else if diameter && across {
        laid.arrow(tip, way, ARROW);
        laid.arrow(c - way * r, -way, ARROW);
        (c - way * r, tip)
    } else {
        // Inside: from the centre — or, where the text stands well out
        // from it, from just past the text — out to the arrow; a
        // diameter of an arc through the centre a little past it.
        laid.arrow(tip, way, ARROW);
        let start = if diameter {
            -(2.0 * ARROW).min(r)
        } else if s - half > r * 0.3 {
            s - half
        } else {
            0.0
        };
        (c + way * start.min(s - half), tip)
    };
    laid.line(from, to, Pen::Narrow);
    // The text on the line, as far along it as it was put (ISO 129-1:
    // the leader is straight, no second bend to the text).
    let foot = c + way * s;
    let text = dimension_text(laid, foot, way, said);
    Drawn::led(text, (from, to))
}

thread_local! {
    /// Whether the sheet being laid out writes the text of a radius or
    /// a diameter on a leader along the leader, not level on a shoulder.
    static ALIGNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A hole's callout: its lines, one under the other, their first at
/// `at`, on a leader that ends in an arrowhead at `tip` on the hole's
/// rim (or wall), with a short shoulder under the text (ISO 129-1).
/// The signs (depth, counterbore, countersink) are drawn, not set as
/// letters: the PDF's fonts have none.
fn callout(laid: &mut Laid, tip: Point2, at: Point2, lines: &[Vec<Callout>]) -> Drawn {
    let h = lettering();
    let sign = h * 0.9;
    let length = |line: &[Callout]| -> f64 {
        line.iter()
            .map(|piece| match piece {
                Callout::Count(_) | Callout::Text(_) | Callout::Tolerance { .. } => {
                    width(&piece.said(), h)
                }
                Callout::Number(x) => width(&number(*x), h),
                _ => sign + h * 0.3,
            })
            .sum()
    };
    let wide = lines.iter().map(|l| length(l)).fold(0.0, f64::max);
    // (The text on the side of `at` away from the tip: left-aligned to
    // its right, right-aligned to its left.)
    let right = at.x >= tip.x;
    let x0 = if right { at.x } else { at.x - wide };
    let mut lo = Point2::new(x0, at.y - OFF);
    let mut hi = Point2::new(x0 + wide, at.y + OFF + h);
    for (k, line) in lines.iter().enumerate() {
        let base = at.y + OFF - k as f64 * h * 1.6;
        let mut x = x0;
        for piece in line {
            let words = match piece {
                Callout::Count(_) | Callout::Text(_) | Callout::Tolerance { .. } => {
                    Some(piece.said())
                }
                Callout::Number(n) => Some(number(*n)),
                _ => None,
            };
            match words {
                Some(text) => {
                    let w = width(&text, h);
                    laid.text(Point2::new(x, base), text, h, [0.0, 0.0]);
                    x += w;
                }
                None => {
                    callout_sign(laid, piece, Point2::new(x + h * 0.15, base), sign);
                    x += sign + h * 0.3;
                }
            }
        }
        lo.y = lo.y.min(base - OFF);
        hi.y = hi.y.max(base + h);
    }
    // The shoulder under the first line, and the leader to the tip.
    let shoulder = if right {
        Point2::new(x0, at.y)
    } else {
        Point2::new(x0 + wide, at.y)
    };
    let end = if right {
        Point2::new(x0 + wide, at.y)
    } else {
        Point2::new(x0, at.y)
    };
    laid.line(shoulder, end, Pen::Narrow);
    laid.line(shoulder, tip, Pen::Narrow);
    laid.arrow(tip, tip - shoulder, ARROW);
    Drawn {
        body: [lo.min(tip), hi.max(tip)],
        ..Drawn::led([lo, hi], (shoulder, tip))
    }
}

/// The sign `of` of a callout (ISO 129-1), `size` high, its lower left
/// at `at`.
fn callout_sign(laid: &mut Laid, of: &Callout, at: Point2, size: f64) {
    let p = |x: f64, y: f64| at + Point2::new(x, y) * size;
    let pen = Pen::Narrow;
    match of {
        // ↧: a stem down onto a bar, an arrowhead on it.
        Callout::Depth => {
            laid.line(p(0.5, 1.0), p(0.5, 0.0), pen);
            laid.line(p(0.15, 0.0), p(0.85, 0.0), pen);
            laid.line(p(0.25, 0.35), p(0.5, 0.0), pen);
            laid.line(p(0.75, 0.35), p(0.5, 0.0), pen);
        }
        // ⌴: a box open at the top.
        Callout::Counterbore => {
            laid.line(p(0.1, 0.9), p(0.1, 0.1), pen);
            laid.line(p(0.1, 0.1), p(0.9, 0.1), pen);
            laid.line(p(0.9, 0.1), p(0.9, 0.9), pen);
        }
        // ⌵: a V, 90° open.
        Callout::Countersink => {
            laid.line(p(0.05, 0.9), p(0.5, 0.45), pen);
            laid.line(p(0.5, 0.45), p(0.95, 0.9), pen);
        }
        Callout::Count(_) | Callout::Text(_) | Callout::Number(_) | Callout::Tolerance { .. } => {}
    }
}

/// The leader of words in the box `rect` to `tip` (ISO 128-22): from
/// the side of the words nearest it, ending in an arrowhead on an
/// outline (one of `lines`), in a dot within a face.
fn words_leader(laid: &mut Laid, rect: [Point2; 2], tip: Point2, lines: &[(Point2, Point2)]) {
    let middle = (rect[0] + rect[1]) / 2.0;
    let from = if tip.x < rect[0].x {
        Point2::new(rect[0].x - OFF, middle.y)
    } else if tip.x > rect[1].x {
        Point2::new(rect[1].x + OFF, middle.y)
    } else if tip.y < rect[0].y {
        Point2::new(middle.x, rect[0].y - OFF)
    } else {
        Point2::new(middle.x, rect[1].y + OFF)
    };
    if from.distance(tip) < 1e-6 {
        return;
    }
    laid.line(from, tip, Pen::Narrow);
    let on_outline = lines.iter().any(|(a, b)| {
        let d = *b - *a;
        let t = ((tip - *a).dot(d) / d.length_squared().max(1e-300)).clamp(0.0, 1.0);
        (*a + d * t).distance(tip) <= 0.3
    });
    if on_outline {
        laid.arrow(tip, tip - from, ARROW);
    } else {
        // (A dot about five times as wide as the line.)
        let r = 0.6;
        let corners: Vec<Point2> = (0..12)
            .map(|k| tip + from_angle(f64::from(k) * std::f64::consts::TAU / 12.0) * r)
            .collect();
        for k in 1..11 {
            laid.fills.push(super::Fill {
                corners: [corners[0], corners[k], corners[k + 1]],
                colour: [0, 0, 0],
            });
        }
    }
}

/// The symbol of the characteristic `of` (ISO 1101), drawn about
/// `centre` in a cell `size` high.
fn symbol(laid: &mut Laid, of: Characteristic, centre: Point2, size: f64) {
    let s = size * 0.3;
    let at = |x: f64, y: f64| centre + Point2::new(x, y) * s;
    let pen = Pen::Narrow;
    match of {
        Characteristic::Straightness => laid.line(at(-1.0, 0.0), at(1.0, 0.0), pen),
        Characteristic::Flatness => {
            let c = [at(-1.0, -0.5), at(0.6, -0.5), at(1.0, 0.5), at(-0.6, 0.5)];
            for k in 0..4 {
                laid.line(c[k], c[(k + 1) % 4], pen);
            }
        }
        Characteristic::Circularity => laid.circle(centre, s * 0.85, pen),
        Characteristic::Cylindricity => {
            laid.circle(centre, s * 0.6, pen);
            laid.line(at(-1.1, -0.9), at(-0.4, 0.9), pen);
            laid.line(at(0.4, -0.9), at(1.1, 0.9), pen);
        }
        Characteristic::LineProfile => {
            laid.arc(at(0.0, -0.5), s, 0.0, std::f64::consts::PI, pen);
        }
        Characteristic::SurfaceProfile => {
            laid.arc(at(0.0, -0.5), s, 0.0, std::f64::consts::PI, pen);
            laid.line(at(-1.0, -0.5), at(1.0, -0.5), pen);
        }
        Characteristic::Parallelism => {
            laid.line(at(-0.8, -0.9), at(-0.1, 0.9), pen);
            laid.line(at(0.1, -0.9), at(0.8, 0.9), pen);
        }
        Characteristic::Perpendicularity => {
            laid.line(at(-1.0, -0.8), at(1.0, -0.8), pen);
            laid.line(at(0.0, -0.8), at(0.0, 0.9), pen);
        }
        Characteristic::Angularity => {
            laid.line(at(-1.0, -0.7), at(1.0, -0.7), pen);
            laid.line(at(-1.0, -0.7), at(0.8, 0.8), pen);
        }
        Characteristic::Position => {
            laid.circle(centre, s * 0.6, pen);
            laid.line(at(-1.1, 0.0), at(1.1, 0.0), pen);
            laid.line(at(0.0, -1.1), at(0.0, 1.1), pen);
        }
        Characteristic::Concentricity => {
            laid.circle(centre, s * 0.9, pen);
            laid.circle(centre, s * 0.45, pen);
        }
        Characteristic::Symmetry => {
            laid.line(at(-1.0, 0.0), at(1.0, 0.0), pen);
            laid.line(at(-0.6, 0.55), at(0.6, 0.55), pen);
            laid.line(at(-0.6, -0.55), at(0.6, -0.55), pen);
        }
        Characteristic::CircularRunout => {
            laid.line(at(-0.8, -0.8), at(0.7, 0.7), pen);
            laid.arrow(at(0.8, 0.8), Point2::new(1.0, 1.0), s * 0.9);
        }
        Characteristic::TotalRunout => {
            for x in [-0.5, 0.5] {
                laid.line(at(x - 0.6, -0.8), at(x + 0.5, 0.7), pen);
                laid.arrow(at(x + 0.6, 0.8), Point2::new(1.0, 1.0), s * 0.9);
            }
            laid.line(at(-1.1, -0.8), at(-0.1, -0.8), pen);
        }
    }
}

/// A leader from the frame (or box) `rect` to the feature at `on`,
/// which the view sees with the way out of the body as `out` on the
/// sheet: seen edge on, it ends square on the outline with `end`
/// drawn there; seen face on, in a dot on the face.
fn leader(laid: &mut Laid, rect: [Point2; 2], on: Point2, out: Point2, triangle: bool) -> Point2 {
    let middle = (rect[0] + rect[1]) / 2.0;
    let edge_on = out.length() > 0.5;
    if !edge_on {
        // On the face itself: a leader that ends in a dot.
        let from = Point2::new(
            if on.x < middle.x {
                rect[0].x
            } else {
                rect[1].x
            },
            middle.y,
        );
        laid.line(from, on, Pen::Narrow);
        laid.circle(on, 0.5, Pen::Narrow);
        laid.fills.push(super::Fill {
            corners: [
                on + Point2::new(0.5, 0.0),
                on + Point2::new(-0.25, 0.43),
                on + Point2::new(-0.25, -0.43),
            ],
            colour: [0, 0, 0],
        });
        return from;
    }
    let out = out.normalize();
    // Square off the outline to the frame (or box): onto the side of it
    // that faces the feature where the frame stands across from it,
    // else to the height (or the width) of its middle and on to the
    // middle of its nearer end (ISO 1101: from an end of the frame;
    // ISO 5459: to a side of the datum's square) — never into it.
    let upright = out.y.abs() >= out.x.abs();
    // (In the frame's own terms: `a` along the way out, `b` across it.)
    let ab = |p: Point2| if upright { (p.y, p.x) } else { (p.x, p.y) };
    let point = |a: f64, b: f64| {
        if upright {
            Point2::new(b, a)
        } else {
            Point2::new(a, b)
        }
    };
    let ((a0, b0), (a1, b1)) = (ab(rect[0]), ab(rect[1]));
    let (on_a, _) = ab(on);
    let way = if upright { out.y } else { out.x };
    let reach = |to: f64| ((to - on_a) / way).max(ARROW * 2.0);
    let side = if way > 0.0 { a0 } else { a1 };
    let straight = on + out * reach(side);
    // (How far out the frame's near side and its middle are: where the
    // frame stands behind the feature, or beside it no farther out than
    // a short way, the leader goes a short way out square off the
    // outline and on to the middle of the frame's nearer end — of a
    // datum's square, of its nearer side.)
    let out_to = |to: f64| (to - on_a) / way;
    let (knee, to) = if out_to(side) >= ARROW * 2.0 - 1e-9 && (b0..=b1).contains(&ab(straight).1) {
        (straight, straight)
    } else if out_to(side) > 0.0 && (b0..=b1).contains(&ab(on).1) {
        // (Right across from it, nearer than that: straight on to it.)
        let there = on + out * out_to(side);
        (there, there)
    } else if out_to((a0 + a1) / 2.0) >= ARROW + 1.0 {
        let knee = on + out * out_to((a0 + a1) / 2.0);
        let (ka, kb) = ab(knee);
        (knee, point(ka, if kb < b0 { b0 } else { b1 }))
    } else {
        let knee = on + out * (ARROW * 2.0);
        let mid = (rect[0] + rect[1]) / 2.0;
        let mut ends = vec![Point2::new(rect[0].x, mid.y), Point2::new(rect[1].x, mid.y)];
        if triangle {
            ends.extend([Point2::new(mid.x, rect[0].y), Point2::new(mid.x, rect[1].y)]);
        }
        let to = ends
            .into_iter()
            .min_by(|p, q| p.distance(knee).total_cmp(&q.distance(knee)))
            .unwrap_or(mid);
        (knee, to)
    };
    if triangle {
        // A datum's triangle on the outline, its line from the apex.
        let h = lettering();
        let base = out.perp() * (h * 0.58);
        laid.fills.push(super::Fill {
            corners: [on + base, on - base, on + out * h],
            colour: [0, 0, 0],
        });
        laid.line(on + out * h, knee, Pen::Narrow);
    } else {
        laid.line(on, knee, Pen::Narrow);
        laid.arrow(on, -out, ARROW);
    }
    if to.distance(knee) > 1e-6 {
        laid.line(knee, to, Pen::Narrow);
    }
    knee
}

/// The graphical symbol of a surface texture (ISO 21920-1, ISO 1302
/// annex A) with its apex at `apex`, for lettering `h` high: two legs
/// 60° off the surface, the shorter 1,4 h high, the longer 3 h with a
/// line on from its top over the requirement `text` (position a); a
/// bar across the legs where material is to be removed, a circle in
/// them where it is not. The box it takes.
pub(crate) fn texture_symbol(
    laid: &mut Laid,
    apex: Point2,
    h: f64,
    text: &str,
    removal: workpiece_core::world::Removal,
) -> [Point2; 2] {
    use workpiece_core::world::Removal;
    let (h1, h2) = (1.4 * h, 3.0 * h);
    let lean = libm::tan((30.0_f64).to_radians());
    let short = apex + Point2::new(-h1 * lean, h1);
    let long = apex + Point2::new(h2 * lean, h2);
    laid.line(apex, short, Pen::Narrow);
    laid.line(apex, long, Pen::Narrow);
    match removal {
        Removal::Any => {}
        Removal::Required => laid.line(short, apex + Point2::new(h1 * lean, h1), Pen::Narrow),
        Removal::Prohibited => {
            let (c, r) = (apex + Point2::new(0.0, h1 / 1.5), h1 / 3.0);
            let at = |k: u32| c + from_angle(std::f64::consts::TAU * f64::from(k) / 32.0) * r;
            for k in 0..32 {
                laid.line(at(k), at(k + 1), Pen::Narrow);
            }
        }
    }
    let mut hi = long;
    if !text.is_empty() {
        // (Under the line on from the longer leg, clear of it.)
        let base = apex.y + h2 - h - 1.0;
        let x = apex.x + (base - apex.y + h) * lean + 0.6;
        let wide = width(text, h);
        laid.line(long, Point2::new(x + wide + 0.6, long.y), Pen::Narrow);
        laid.text(Point2::new(x, base), text.to_owned(), h, [0.0, 0.0]);
        hi = Point2::new(x + wide + 0.6, long.y);
    }
    [Point2::new(short.x, apex.y), hi]
}

/// A box with lines of the narrow pen.
fn boxed(laid: &mut Laid, lo: Point2, hi: Point2) {
    let c = [lo, Point2::new(hi.x, lo.y), hi, Point2::new(lo.x, hi.y)];
    for k in 0..4 {
        laid.line(c[k], c[(k + 1) % 4], Pen::Narrow);
    }
}

/// Draws the annotation `note` in the view `placed` with its text at
/// `at` on the sheet; gives the box of its text, or nothing if the
/// view does not see it true.
pub(crate) fn draw_note(
    laid: &mut Laid,
    note: &Annotation,
    placed: &dyn Frame,
    at: Point2,
    starts: [f64; 2],
    attach: &Attach,
) -> Option<Drawn> {
    // (Measured where this view sees it true.)
    let note = &*as_seen(note, placed, Some(at))?;
    let towards = placed.towards();
    let scale = placed.scale();
    match &note.note {
        Note::Texture {
            text,
            removal,
            on,
            out,
        } => {
            // On the outline where the face looks up and the symbol is
            // put right over it; else on a leader to it, ending in an
            // arrow on the outline (ISO 1302, ISO 21920-1).
            let rect = [at - Point2::splat(1.0), at + Point2::splat(1.0)];
            let (tip, way) = feature_tip(placed, *on, *out, None, rect, attach);
            let up = way.try_normalize().is_some_and(|w| w.y > 0.95);
            if up && (at.x - tip.x).abs() < 1.0 && at.y >= tip.y - 1e-6 {
                let body = texture_symbol(laid, tip, lettering(), text, *removal);
                return Some(Drawn {
                    leader: Some((tip, tip)),
                    ..Drawn::of(body)
                });
            }
            let body = texture_symbol(laid, at, lettering(), text, *removal);
            let h = lettering();
            // (Its reference line under the symbol, the leader from
            // its nearer end.)
            let (left, right) = (
                at - Point2::new(h * 0.9, 0.0),
                at + Point2::new(h * 1.9, 0.0),
            );
            laid.line(left, right, Pen::Narrow);
            let from = if (tip - left).length() < (tip - right).length() {
                left
            } else {
                right
            };
            laid.line(from, tip, Pen::Narrow);
            if way.length() > 0.5 {
                laid.arrow(tip, tip - from, ARROW);
            } else {
                // (On the face, seen square on: a dot.)
                laid.circle(tip, 0.5, Pen::Narrow);
                laid.fills.push(super::Fill {
                    corners: [
                        tip + Point2::new(0.5, 0.0),
                        tip + Point2::new(-0.25, 0.43),
                        tip + Point2::new(-0.25, -0.43),
                    ],
                    colour: [0, 0, 0],
                });
            }
            Some(Drawn {
                body: [body[0].min(tip), body[1].max(tip)],
                ..Drawn::led(body, (tip, from))
            })
        }
        Note::Dimension {
            measured,
            value,
            tolerance,
            ..
        } => match measured {
            Measured::Length { from, to, along } => {
                let ((p, q), along) = length_ends(placed, *from, *to, *along)?;
                // Each extension line from the outline: where the view
                // draws what it measures, the end of that nearest the
                // dimension line.
                let across = along.perp();
                let (mut unseen, mut loose) = (false, false);
                let mut start =
                    |x: Point2| match placed.outline(x, x + across * (at - x).dot(across)) {
                        Outline::Drawn(there) => there,
                        Outline::Hidden => {
                            unseen = true;
                            x
                        }
                        Outline::Free => {
                            loose = true;
                            x
                        }
                    };
                // (Of a half dimension, the end not shown has none.)
                let half = halved(placed, (p, q), along, at);
                let (p, q) = match half {
                    Some((0, _)) => (p, start(q)),
                    Some(_) => (start(p), q),
                    None => (start(p), start(q)),
                };
                let drawn = linear(
                    laid,
                    (p, q),
                    along,
                    at,
                    &said(*value, "", "", tolerance),
                    starts,
                    half,
                );
                Some(Drawn {
                    unseen,
                    loose,
                    ..drawn
                })
            }
            Measured::Diameter {
                centre,
                axis,
                radius,
            } => {
                let c = placed.place(*centre);
                let r = radius * scale;
                let text = said(*value, "Ø", "", tolerance);
                if square_on(*axis, towards) {
                    // (A round the view hides and does not draw — a tube
                    // behind its flange: its arrows point at nothing.)
                    let wanted = (at - c).normalize_or(Point2::X);
                    let unseen = placed.on_arc(c, r, wanted).is_none()
                        && placed.on_arc(c, r, -wanted).is_none()
                        && placed.hidden_arc(c, r, wanted).is_some();
                    let drawn = round_dimension(laid, placed, c, r, at, &text, true, None);
                    Some(Drawn { unseen, ..drawn })
                } else {
                    // From the side: across it, as a length (a half
                    // dimension where one side is not shown: across
                    // the axis of a half section).
                    let across = placed.way(*axis).try_normalize()?.perp();
                    let ends = (c + across * r, c - across * r);
                    let half = halved(placed, ends, across, at);
                    Some(linear(laid, ends, across, at, &text, starts, half))
                }
            }
            Measured::Radius {
                centre,
                radius,
                axis,
            } if axis.dot(towards).abs() < SLANT => {
                // From the side: from its axis out to it, as a length
                // — on the side the text stands.
                let c = placed.place(*centre);
                let across = placed.way(*axis).try_normalize()?.perp();
                let side = if (at - c).dot(across) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                Some(linear(
                    laid,
                    (c, c + across * (radius * scale * side)),
                    across,
                    at,
                    &said(*value, "R", "", tolerance),
                    starts,
                    None,
                ))
            }
            Measured::Radial {
                centre,
                axis,
                inner,
                outer,
            } => {
                // From the one radius to the other: towards the text
                // seen square on, across the axis from the side.
                let c = placed.place(*centre);
                let way = if square_on(*axis, towards) {
                    (at - c).try_normalize().unwrap_or(Point2::X)
                } else {
                    let across = placed.way(*axis).try_normalize()?.perp();
                    if (at - c).dot(across) < 0.0 {
                        -across
                    } else {
                        across
                    }
                };
                Some(linear(
                    laid,
                    (c + way * (inner * scale), c + way * (outer * scale)),
                    way,
                    at,
                    &said(*value, "", "", tolerance),
                    starts,
                    None,
                ))
            }
            Measured::Radius {
                centre,
                radius,
                axis,
            } => {
                let c = placed.place(*centre);
                // Seen slantwise — an arc of a drafted body — its arrow
                // where the arc is seen, the way of the text; the true
                // radius said, as such ("TRUE R20").
                let (prefix, given) = if square_on(*axis, towards) {
                    ("R", None)
                } else {
                    let u = axis.any_orthonormal_vector();
                    let v = axis.cross(u);
                    let wanted = (at - c).try_normalize().unwrap_or(Point2::X);
                    let tip = (0..720)
                        .map(|k| {
                            let a = f64::from(k) * std::f64::consts::TAU / 720.0;
                            placed.place(*centre + (u * libm::cos(a) + v * libm::sin(a)) * *radius)
                        })
                        .max_by(|p, q| {
                            let d = |x: &Point2| (*x - c).normalize_or_zero().dot(wanted);
                            d(p).total_cmp(&d(q))
                        });
                    ("TRUE R", tip)
                };
                let text = said(*value, prefix, "", tolerance);
                // (A round the view hides and does not draw: it points at
                // nothing there.)
                let wanted = (at - c).normalize_or(Point2::X);
                let unseen = given.is_none()
                    && placed.on_arc(c, radius * scale, wanted).is_none()
                    && placed.hidden_arc(c, radius * scale, wanted).is_some();
                let drawn =
                    round_dimension(laid, placed, c, radius * scale, at, &text, false, given);
                Some(Drawn { unseen, ..drawn })
            }
            Measured::Angle {
                at: vertex,
                first,
                second,
            } => {
                let v = placed.place(*vertex);
                let (d1, d2) = (
                    placed.way(*first).try_normalize()?,
                    placed.way(*second).try_normalize()?,
                );
                // Of the four sectors the two lines make, the one the
                // dimension stands in: of the two as wide as its value,
                // the nearer the text.
                let u = (at - v)
                    .try_normalize()
                    .unwrap_or((d1 + d2).try_normalize().unwrap_or(d1.perp()));
                let between = |a: Point2, b: Point2| libm::acos(a.dot(b).clamp(-1.0, 1.0));
                let geometric = between(d1, d2).to_degrees();
                let as_is = (geometric - value).abs() <= (geometric - (180.0 - value)).abs();
                // (An angle says its value — what a sketch or a step
                // holds, what was measured, or what is left of a half
                // turn where that was asked — never the other: of the
                // sectors only the two that have it, the text picks the
                // nearer, standing outside it on the arc run on where it
                // is put outside.)
                let sectors: Vec<(f64, f64)> = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)]
                    .into_iter()
                    .filter(|(s1, s2)| (s1 * s2 > 0.0) == as_is)
                    .collect();
                let (s1, s2) = sectors
                    .into_iter()
                    .min_by(|x, y| {
                        let miss = |(s1, s2): (f64, f64)| {
                            let (e1, e2) = (d1 * s1, d2 * s2);
                            between(e1, u) + between(u, e2) - between(e1, e2)
                        };
                        miss(*x).total_cmp(&miss(*y))
                    })
                    .unwrap_or((1.0, 1.0));
                let (e1, e2) = (d1 * s1, d2 * s2);
                let shown = *value;
                // The arc through where the text is put, about the
                // vertex, from the one arm to the other.
                let radius = at.distance(v).max(ARROW * 3.0);
                let (a1, a2) = (libm::atan2(e1.y, e1.x), libm::atan2(e2.y, e2.x));
                let mut sweep = a2 - a1;
                while sweep > std::f64::consts::PI {
                    sweep -= std::f64::consts::TAU;
                }
                while sweep < -std::f64::consts::PI {
                    sweep += std::f64::consts::TAU;
                }
                let turn = if sweep < 0.0 { -1.0 } else { 1.0 };
                let wide = sweep.abs();
                // (How far round from the first arm, the way to the
                // second, a direction is: between -π and π.)
                let round = |d: Point2| {
                    let mut t = (libm::atan2(d.y, d.x) - a1) * turn;
                    while t > std::f64::consts::PI {
                        t -= std::f64::consts::TAU;
                    }
                    while t <= -std::f64::consts::PI {
                        t += std::f64::consts::TAU;
                    }
                    t
                };
                let text = said(shown, "", "°", tolerance);
                // (Room along the arc, as angles: half the text's, an
                // arrow's.)
                let half = (text.room() / 2.0 + 0.5) / radius;
                let arrow = ARROW / radius;
                // Arrows within the arc where they fit, else from
                // outside; the text between them where it fits too, else
                // out past an arm (ISO 129-1).
                let inside = wide * radius >= 2.0 * ARROW + 2.0;
                let fits = wide * radius >= text.room() + 2.0 * ARROW + 2.0;
                let clear = if inside {
                    0.5 / radius
                } else {
                    arrow + 1.0 / radius
                };
                // Its text where it is put along the arc: within the
                // sector, clear of the arrows, where there is room;
                // else out past the arm nearer, on the arc run on.
                let put = round(u);
                // (Too long to stand along the arc between its arrows —
                // a value with its tolerance — but put within the
                // sector: over the arc's middle, outside it, clear of
                // the arms and of what runs on past them.)
                let over_middle = !fits && inside && (0.0..=wide).contains(&put);
                let put = if over_middle {
                    wide / 2.0
                } else if (0.0..=wide).contains(&put) {
                    if fits {
                        put.clamp(half + arrow, (wide - half - arrow).max(half + arrow))
                    } else if put < wide / 2.0 {
                        -(clear + half)
                    } else {
                        wide + clear + half
                    }
                } else if put < 0.0 && put > wide / 2.0 - std::f64::consts::PI {
                    // (Nearer past the first arm.)
                    put.min(-(clear + half))
                } else {
                    // (Nearer past the second.)
                    let put = if put < 0.0 {
                        put + std::f64::consts::TAU
                    } else {
                        put
                    };
                    put.max(wide + clear + half)
                };
                let (lo, hi) = (put.min(0.0), put.max(wide));
                let (lo, hi) = if inside {
                    (lo, hi)
                } else {
                    (lo.min(-2.0 * arrow), hi.max(wide + 2.0 * arrow))
                };
                laid.arc(v, radius, a1 + lo * turn, (hi - lo) * turn, Pen::Narrow);
                // The extension lines: from the edges (a gap off them)
                // on a little past the arc, where the arc does not meet
                // the edges themselves — across the apex too, where it
                // lies off the part.
                for (k, e) in [e1, e2].into_iter().enumerate() {
                    match placed.along(v, e) {
                        Some((_, hi)) if radius > hi => laid.line(
                            v + e * (hi + starts[k]),
                            v + e * (radius + PAST),
                            Pen::Narrow,
                        ),
                        Some((lo, _)) if radius < lo => laid.line(
                            v + e * (radius - PAST).max(0.0),
                            v + e * (lo - starts[k]),
                            Pen::Narrow,
                        ),
                        Some(_) => {}
                        None => laid.line(v, v + e * (radius + PAST), Pen::Narrow),
                    }
                }
                let sign = if inside { 1.0 } else { -1.0 };
                laid.arrow(v + e1 * radius, -e1.perp() * (turn * sign), ARROW);
                laid.arrow(v + e2 * radius, e2.perp() * (turn * sign), ARROW);
                let u = from_angle(a1 + put * turn);
                // (Over the middle, its foot outside the arc: where it
                // would read standing inwards, moved out by its height.)
                let lift = if over_middle {
                    let along =
                        if u.perp().x < -1e-9 || (u.perp().x.abs() <= 1e-9 && u.perp().y < 0.0) {
                            -u.perp()
                        } else {
                            u.perp()
                        };
                    if along.perp().dot(u) < 0.0 {
                        text.high.max(lettering()) + 2.0 * OFF
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                Some(Drawn::of(dimension_text(
                    laid,
                    v + u * (radius + lift),
                    u.perp(),
                    &text,
                )))
            }
        },
        Note::Hole {
            centre,
            axis,
            radius,
            lines,
            ..
        } => {
            let c = placed.place(*centre);
            // (Its rims: the bore's, and a counterbore's — the leader
            // ends on one the view draws, the bore's first.)
            let head = lines.iter().find_map(|line| {
                let k = line.iter().position(|p| *p == Callout::Counterbore)?;
                line[k..].iter().find_map(|p| match p {
                    Callout::Number(d) => Some(d / 2.0),
                    _ => None,
                })
            });
            let radii: Vec<f64> = std::iter::once(*radius)
                .chain(head)
                .map(|r| r * scale)
                .collect();
            let tip = if square_on(*axis, towards) {
                // On the rim where it is seen as a circle.
                let way = (at - c)
                    .try_normalize()
                    .unwrap_or(Point2::new(1.0, 1.0).normalize());
                radii
                    .iter()
                    .find_map(|r| placed.on_arc(c, *r, way))
                    .unwrap_or(c + way * radii[0])
            } else {
                // From the side: on its wall, on the side of the text,
                // at its mouth — or, where nothing is drawn there (its
                // mouth in the open, a counterbore's floor up to a face
                // further in), where the wall the view draws is nearest
                // it.
                let along = placed.way(*axis).try_normalize()?;
                let across = along.perp();
                let side = if (at - c).dot(across) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let mouth = |r: f64| c + across * (r * side);
                radii
                    .iter()
                    .find_map(|r| {
                        let (lo, hi) = placed.along(mouth(*r), along)?;
                        let t = 0.0_f64.clamp(lo, hi);
                        Some(mouth(*r) + along * t)
                    })
                    .unwrap_or(mouth(radii[0]))
            };
            Some(callout(laid, tip, at, lines))
        }
        Note::Datum {
            letter,
            on,
            out,
            axis,
        } => {
            // Its letter in a square, its triangle on the feature — of
            // an axis, on the extension of its diameter's dimension line.
            let side = lettering() * 2.0;
            let half = Point2::splat(side / 2.0)
                .max(Point2::new(width(letter, lettering()) / 2.0 + 1.0, 0.0));
            let rect = [at - half, at + half];
            boxed(laid, rect[0], rect[1]);
            laid.text(at, letter.clone(), lettering(), [0.5, 0.5]);
            let (tip, way) = feature_tip(placed, *on, *out, axis.as_ref(), rect, attach);
            let knee = leader(laid, rect, tip, way, true);
            Some(Drawn::led(rect, (tip, knee)))
        }
        Note::Geometric {
            characteristic,
            value,
            diametral,
            datums,
            on,
            out,
            axis,
        } => {
            // The frame: the symbol, the tolerance, each datum, in
            // compartments twice as high as the lettering.
            let high = lettering() * 2.0;
            let zone = format!("{}{}", if *diametral { "Ø" } else { "" }, number(*value));
            let mut cells = vec![high, width(&zone, lettering()) + 3.0];
            cells.extend(datums.iter().map(|d| width(d, lettering()) + 3.0));
            let whole: f64 = cells.iter().sum();
            let lo = at - Point2::new(whole / 2.0, high / 2.0);
            let rect = [lo, lo + Point2::new(whole, high)];
            boxed(laid, rect[0], rect[1]);
            let mut x = lo.x;
            for (k, wide) in cells.iter().enumerate() {
                if k > 0 {
                    laid.line(
                        Point2::new(x, lo.y),
                        Point2::new(x, lo.y + high),
                        Pen::Narrow,
                    );
                }
                let middle = Point2::new(x + wide / 2.0, lo.y + high / 2.0);
                match k {
                    0 => symbol(laid, *characteristic, middle, high),
                    1 => laid.text(middle, zone.clone(), lettering(), [0.5, 0.5]),
                    _ => laid.text(middle, datums[k - 2].clone(), lettering(), [0.5, 0.5]),
                }
                x += wide;
            }
            // (Of an axis, the arrow on the extension of its diameter's
            // dimension line, in line with it.)
            let (tip, way) = feature_tip(placed, *on, *out, axis.as_ref(), rect, attach);
            let knee = leader(laid, rect, tip, way, false);
            Some(Drawn::led(rect, (tip, knee)))
        }
    }
}

/// A hole's callout `note` as `sheet` writes it: ` THRU` after a
/// hole through where the sheet says so (ASME Y14.5); how many holes
/// alike it is of — `count` where the sheet's note says (0: none), else
/// as the model has it — as `6× ` before it, or (the sheet's `places`)
/// as a line `(6) PLACES` after it.
fn callout_as_written(note: &Annotation, sheet: &Sheet, count: Option<u32>) -> Annotation {
    use workpiece_core::world::Callout;
    let mut said = note.clone();
    let Note::Hole { lines, outside, .. } = &mut said.note else {
        return said;
    };
    // (A rod's thread is through nothing.)
    let outside = *outside;
    let Some(first) = lines.first_mut() else {
        return said;
    };
    let had = first.iter().find_map(|c| match c {
        Callout::Count(n) => Some(*n),
        _ => None,
    });
    first.retain(|c| !matches!(c, Callout::Count(_)));
    // (A depth that is no typed number — of a floor up to a face — is a
    // reference: written, in brackets, only where the sheet asks.)
    if !sheet.reference_depths {
        for line in lines.iter_mut() {
            while let Some(k) = line.windows(4).position(|w| {
                w[0] == Callout::Depth
                    && w[1] == Callout::Text("(".into())
                    && matches!(w[2], Callout::Number(_))
                    && w[3] == Callout::Text(")".into())
            }) {
                let from = if k > 0 && line[k - 1] == Callout::Text(" ".into()) {
                    k - 1
                } else {
                    k
                };
                line.drain(from..k + 4);
            }
        }
    }
    let Some(first) = lines.first_mut() else {
        return said;
    };
    let thru = Callout::Text(" THRU".into());
    if sheet.thru && !outside && !first.contains(&Callout::Depth) && !first.contains(&thru) {
        first.push(thru);
    }
    let n = count.map_or(had.unwrap_or(1), |n| n as usize);
    if n > 1 {
        if sheet.places {
            lines.push(vec![Callout::Text(format!("({n}) PLACES"))]);
        } else {
            first.insert(0, Callout::Count(n));
        }
    }
    said
}

/// Cuts the lines of `laid` from the `start`-th on back to the box
/// from `lo` to `hi`: what of them is outside goes.
fn kept_within(laid: &mut Laid, start: usize, lo: Point2, hi: Point2) {
    let own = laid.strokes.split_off(start.min(laid.strokes.len()));
    for mut stroke in own {
        let d = stroke.b - stroke.a;
        // (Liang–Barsky.)
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        let mut inside = true;
        for (p, q) in [
            (-d.x, stroke.a.x - lo.x),
            (d.x, hi.x - stroke.a.x),
            (-d.y, stroke.a.y - lo.y),
            (d.y, hi.y - stroke.a.y),
        ] {
            if p == 0.0 {
                inside &= q >= 0.0;
            } else if p < 0.0 {
                t0 = t0.max(q / p);
            } else {
                t1 = t1.min(q / p);
            }
        }
        if inside && t0 < t1 {
            let a = stroke.a;
            stroke.a = a + d * t0;
            stroke.b = a + d * t1;
            laid.strokes.push(stroke);
        }
    }
}

/// Whether something round about `axis` is seen square on by who looks
/// along `towards`: along its axis — or it is a ball, which has none.
fn square_on(axis: workpiece_core::math::Point3, towards: workpiece_core::math::Point3) -> bool {
    axis == workpiece_core::math::Point3::ZERO || axis.dot(towards).abs() > 1.0 - 1e-9
}

/// Whether the boxes `a` and `b` are nearer than `gap` to each other.
fn near(a: &[Point2; 2], b: &[Point2; 2], gap: f64) -> bool {
    a[0].x < b[1].x + gap && b[0].x < a[1].x + gap && a[0].y < b[1].y + gap && b[0].y < a[1].y + gap
}

/// Takes out of the narrow lines of `laid` what of them crosses the
/// boxes of texts (ISO 129-1: no line runs through a dimension's
/// value; a line that would is interrupted). Each box comes with the
/// lines that are its own note's, which stay.
fn clear(laid: &mut Laid, texts: &[([Point2; 2], std::ops::Range<usize>)]) {
    if texts.is_empty() {
        return;
    }
    let strokes = std::mem::take(&mut laid.strokes);
    for (index, stroke) in strokes.into_iter().enumerate() {
        if !matches!(stroke.pen, Pen::Narrow | Pen::Chain | Pen::Dashed) {
            laid.strokes.push(stroke);
            continue;
        }
        // The pieces of it outside every box.
        let mut pieces = vec![(0.0_f64, 1.0_f64)];
        let d = stroke.b - stroke.a;
        for (rect, own) in texts {
            if own.contains(&index) {
                continue;
            }
            let (lo, hi) = (rect[0] - Point2::splat(0.4), rect[1] + Point2::splat(0.4));
            // Where the line is inside the box (Liang–Barsky).
            let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
            let mut inside = true;
            for (p, q) in [
                (-d.x, stroke.a.x - lo.x),
                (d.x, hi.x - stroke.a.x),
                (-d.y, stroke.a.y - lo.y),
                (d.y, hi.y - stroke.a.y),
            ] {
                if p.abs() < 1e-12 {
                    inside &= q >= 0.0;
                } else if p < 0.0 {
                    t0 = t0.max(q / p);
                } else {
                    t1 = t1.min(q / p);
                }
            }
            if !inside || t0 >= t1 {
                continue;
            }
            pieces = pieces
                .into_iter()
                .flat_map(|(a, b)| [(a, b.min(t0)), (a.max(t1), b)])
                .filter(|(a, b)| b - a > 1e-9)
                .collect();
        }
        for (a, b) in pieces {
            laid.strokes.push(super::Stroke {
                a: stroke.a + d * a,
                b: stroke.a + d * b,
                ..stroke
            });
        }
    }
}

/// Draws what stands on `sheet` — what is said of the model, and the
/// dimensions of the sheet's own — and counts what is said of the model
/// and stands nowhere. With `previews`, that is drawn as well, as what
/// it would be: in the view named, or in every view that sees it true.
pub(crate) fn draw(
    laid: &mut Laid,
    sheet: &Sheet,
    subject: &Subject<'_>,
    frames: &[Placed<'_>],
    previews: Option<Option<&str>>,
) -> Taken {
    ALIGNED.with(|a| a.set(sheet.aligned_leaders));
    super::views::TRACES.with(|t| {
        *t.borrow_mut() = frames
            .iter()
            .map(|placed| {
                (
                    placed.view.id.clone(),
                    super::lines::traces(subject.world, placed, &sheet.drafting),
                )
            })
            .collect();
    });
    // (The traces dimensions end on, drawn once each.)
    let mut traced: Vec<(Point2, Point2)> = Vec::new();
    // (Of each length drawn: its view, name, value, the way its line
    // runs, where across that the line is, how far it runs along it
    // with its text, and where its arrows are.)
    #[allow(clippy::type_complexity)]
    let mut runs: Vec<(String, String, String, Point2, f64, (f64, f64), (f64, f64))> = Vec::new();
    let notes = &subject.world.annotations;
    // (What stands on the sheet, in any view.)
    let mut on_sheet = Taken::default();
    // (Of each note drawn, where its lettering starts: its value.)
    // (Of each note drawn, the texts it wrote: its value first.)
    let mut values: Vec<(&str, std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
    let mut texts: Vec<([Point2; 2], std::ops::Range<usize>)> = Vec::new();
    let mut taken: std::collections::BTreeMap<&str, Taken> = Default::default();
    // (The diameters drawn from the side, by view: what a datum or a
    // tolerance of an axis attaches to — those are drawn after them.)
    let mut sizes: Vec<(String, SizeLine)> = Vec::new();
    // (Where the leaders of datums and tolerances end, by view.)
    let mut tips: Vec<(String, Point2)> = Vec::new();
    let mut order: Vec<usize> = (0..sheet.notes.len()).collect();
    order.sort_by_key(|&k| {
        subject.world.said(&sheet.notes[k].note).is_some_and(|n| {
            matches!(
                n.note,
                workpiece_core::world::Note::Datum { .. }
                    | workpiece_core::world::Note::Geometric { .. }
            )
        })
    });
    for index in order {
        let placed_note = &sheet.notes[index];
        let Some(placed) = frames.iter().find(|p| p.view.id == placed_note.view) else {
            continue;
        };
        // (Of the model, or of a part placed in it: `part/name`.)
        let Some(note) = subject.world.said(&placed_note.note) else {
            laid.problems.push(format!(
                "{} is no longer said of the model",
                placed_note.note
            ));
            continue;
        };
        let note = &*note;
        let at = placed.centre + Point2::from_array(placed_note.at);
        let start = laid.strokes.len();
        let lettering = laid.texts.len();
        // (Where its arrowheads start.)
        let fills0 = laid.fills.len();
        // (With the decimals said of this one, if any are.)
        let how = workpiece_core::world::annotation::numbers();
        // (Decimals said of this one are as many as it says.)
        let how = workpiece_core::world::Numbers {
            decimals: placed_note.decimals.or(how.decimals),
            trim: how.trim && placed_note.decimals.is_none(),
            ..how
        };
        let gap = sheet.spacing.gap;
        let starts = placed_note.starts.map(|s| s.unwrap_or(gap));
        // (A hole's callout as the sheet writes it: through, where the
        // sheet says so, `Ø6 THRU`; how many holes alike, as the note or
        // the model says, `6×` before it or `(6) PLACES` after it.)
        let written;
        let note = match &note.note {
            workpiece_core::world::Note::Hole { .. } => {
                written = callout_as_written(note, sheet, placed_note.count);
                &written
            }
            _ => note,
        };
        let attach = Attach {
            sizes: sizes
                .iter()
                .filter(|(view, _)| *view == placed.view.id)
                .map(|(_, s)| *s)
                .collect(),
            taken: tips
                .iter()
                .filter(|(view, _)| *view == placed.view.id)
                .map(|(_, p)| *p)
                .collect(),
            to: placed_note
                .to
                .map(|to| placed.centre + Point2::from_array(to)),
        };
        let drawn = workpiece_core::world::annotation::written(how, || {
            draw_note(laid, note, placed, at, starts, &attach)
        });
        if let (
            Some(Drawn {
                leader: Some((tip, _)),
                ..
            }),
            workpiece_core::world::Note::Datum { .. }
            | workpiece_core::world::Note::Geometric { .. },
        ) = (&drawn, &note.note)
        {
            tips.push((placed.view.id.clone(), *tip));
        }
        // (A diameter, or a hole's callout to its rim seen as a circle:
        // what a tolerance of the round's axis stands in line with.)
        let sized = match &note.note {
            workpiece_core::world::Note::Dimension {
                measured:
                    workpiece_core::world::Measured::Diameter {
                        centre,
                        axis,
                        radius,
                    },
                ..
            } => Some((*centre, *axis, *radius)),
            workpiece_core::world::Note::Hole {
                centre,
                axis,
                radius,
                outside: false,
                ..
            } if square_on(*axis, placed.towards()) => Some((*centre, *axis, *radius)),
            _ => None,
        };
        if let (Some(drawn), Some((centre, axis, radius))) = (&drawn, sized) {
            // (Seen square on, its line across the circle through the
            // middle, from the arrow on it: where the leader from the
            // text runs on through the middle to the other side.)
            let (c, r) = (placed.place(centre), radius * placed.scale);
            let line = match (drawn.linear, drawn.leader) {
                (Some(linear), _) => Some(linear.line),
                (None, Some((a, b))) => {
                    let tip = if (a.distance(c) - r).abs() <= (b.distance(c) - r).abs() {
                        a
                    } else {
                        b
                    };
                    ((tip.distance(c) - r).abs() <= r * 1e-3 + 1e-6).then(|| [c * 2.0 - tip, tip])
                }
                (None, None) => None,
            };
            if let Some(line) = line {
                sizes.push((
                    placed.view.id.clone(),
                    SizeLine {
                        centre,
                        axis,
                        radius,
                        line,
                        text: drawn.text,
                    },
                ));
            }
        }
        values.push((
            placed_note.note.as_str(),
            lettering..laid.texts.len(),
            start..laid.strokes.len(),
        ));
        match drawn {
            Some(drawn) => {
                // In a detail, what it measures may lie outside what
                // the detail shows: its lines end at the detail's rim
                // (or at its own text, where that stands outside).
                if let (Some(reach), false) = (placed.reach, drawn.partial) {
                    let (lo, hi) = placed.bounds();
                    // (Its text far off the detail's circle — beside
                    // another view — is told: it belongs in the detail.)
                    let middle = (drawn.text[0] + drawn.text[1]) / 2.0;
                    if middle.distance(placed.centre) > reach * placed.scale + 5.0 {
                        laid.problems.push(format!(
                            "the dimension {} ({}) in the detail {} stands {:.0} mm off the detail's circle: \
                             put its text by the detail (at=)",
                            placed_note.note,
                            note.text(),
                            placed_note.view,
                            middle.distance(placed.centre) - reach * placed.scale
                        ));
                    }
                    let room = Point2::splat(PAST);
                    let (lo, hi) = (lo.min(drawn.text[0]) - room, hi.max(drawn.text[1]) + room);
                    kept_within(laid, start, lo, hi);
                    // (And its arrowheads: one out there stands on
                    // nothing of it.)
                    let mut k = 0;
                    laid.fills.retain(|f| {
                        k += 1;
                        k <= fills0
                            || (f.corners[0].cmpge(lo).all() && f.corners[0].cmple(hi).all())
                    });
                }
                laid.notes.push(LaidNote {
                    index,
                    kind: super::Placing::Note,
                    rect: drawn.text,
                    view: placed.view.id.clone(),
                    preview: None,
                    linear: drawn.linear,
                    path: Vec::new(),
                    tips: Vec::new(),
                });
                // (A line that runs far from what it is of: the text
                // stands too far off it.)
                let long = drawn
                    .linear
                    .map(|l| {
                        (0..2)
                            .map(|k| l.from[k].distance(l.line[k]))
                            .fold(0.0, f64::max)
                    })
                    .into_iter()
                    .chain(drawn.leader.map(|(a, b)| a.distance(b)))
                    .fold(0.0, f64::max);
                if long > LONGEST {
                    laid.problems.push(format!(
                        "the dimension {} ({}) in the view {} has a line {long:.0} mm long, out to where its text \
                         stands: put its text nearer what it measures",
                        placed_note.note,
                        note.text(),
                        placed_note.view
                    ));
                }
                // (Where its line runs, text and all: what a length on
                // the same line beside it must keep off.)
                if let Some(l) = drawn.linear {
                    let way = (l.line[1] - l.line[0]).normalize_or_zero();
                    if way != Point2::ZERO {
                        let (s0, s1) = (l.line[0].dot(way), l.line[1].dot(way));
                        let (t0, t1) = (drawn.text[0].dot(way), drawn.text[1].dot(way));
                        let span = (s0.min(s1).min(t0.min(t1)), s0.max(s1).max(t0.max(t1)));
                        runs.push((
                            placed.view.id.clone(),
                            placed_note.note.clone(),
                            note.text(),
                            way,
                            l.line[0].perp_dot(way),
                            span,
                            (s0.min(s1), s0.max(s1)),
                        ));
                    }
                }
                // (A trace of a datum its extension lines start on is
                // drawn: chain, a little past the outline.)
                if let Some(l) = drawn.linear {
                    super::views::TRACES.with(|t| {
                        for (a, b, pen) in t.borrow().get(&placed.view.id).into_iter().flatten() {
                            let way = (*b - *a).normalize_or_zero();
                            let on = l.from.iter().any(|p| {
                                (*p - *a).perp_dot(way).abs() <= 0.05
                                    && (*p - *a).dot(way) >= -0.05
                                    && (*p - *a).dot(way) <= a.distance(*b) + 0.05
                            });
                            if on && !traced.contains(&(*a, *b)) {
                                laid.line(*a, *b, *pen);
                                traced.push((*a, *b));
                            }
                        }
                    });
                }
                // (What ends on a datum plane or axis the view sees edge
                // on ends on its trace, drawn: a plane's offset.)
                let on_datum = drawn.loose
                    && !drawn.unseen
                    && drawn.linear.is_some_and(|l| {
                        // (And the centre lines and traces drawn in it.)
                        let (lo, hi) = placed.bounds();
                        let (lo, hi) = (lo - Point2::splat(10.0), hi + Point2::splat(10.0));
                        let mut lines = drawn_lines(placed);
                        lines.extend(
                            laid.strokes
                                .iter()
                                .filter(|s| {
                                    s.pen == Pen::Chain
                                        && [s.a, s.b]
                                            .iter()
                                            .all(|p| p.cmpge(lo).all() && p.cmple(hi).all())
                                })
                                .map(|s| (s.a, s.b)),
                        );
                        l.from.iter().all(|p| {
                            lines.iter().any(|(a, b)| {
                                let d = *b - *a;
                                let t = ((*p - *a).dot(d) / d.length_squared().max(1e-300))
                                    .clamp(0.0, 1.0);
                                (*a + d * t).distance(*p) <= 0.05
                            }) || super::lines::datum_through(
                                laid,
                                subject.world,
                                placed,
                                &sheet.drafting,
                                *p,
                            )
                        })
                    });
                // (An end at the middle of an arc the view draws — a
                // bend's — is on what it draws: its centre mark drawn,
                // where no line of the view's crosses there.)
                for p in drawn.linear.map(|l| l.from).into_iter().flatten() {
                    let Some(r) = placed.round_about(p) else {
                        continue;
                    };
                    let marked = laid.strokes.iter().any(|s| {
                        s.pen == Pen::Chain && {
                            let d = s.b - s.a;
                            let t =
                                ((p - s.a).dot(d) / d.length_squared().max(1e-300)).clamp(0.0, 1.0);
                            (s.a + d * t).distance(p) <= 0.05
                        }
                    });
                    if !marked {
                        for (a, b, pen) in super::lines::mark(p, r, &sheet.drafting, [0.0; 2]) {
                            // (Of a detail, within its circle.)
                            if let Some((a, b)) = super::lines::in_circle(placed, a, b) {
                                laid.line(a, b, pen);
                            }
                        }
                    }
                }
                // (A pictorial view — from a corner — shows faces as
                // areas, not edge on: an end on a face it shows, or on
                // what a cut lays open, is in view there, not on a line.)
                let towards = placed.basis[2];
                let pictorial =
                    towards.x.abs().max(towards.y.abs()).max(towards.z.abs()) < 1.0 - 1e-6;
                if drawn.loose && !drawn.unseen && !on_datum && !pictorial {
                    laid.problems.push(format!(
                        "the dimension {} ({}) in the view {} ends on nothing that view draws (its extension \
                         line starts in empty space): measure it between edges or faces the view shows",
                        placed_note.note,
                        note.text(),
                        placed_note.view
                    ));
                }
                if drawn.unseen {
                    laid.problems.push(format!(
                        "the dimension {} ({}) in the view {} ends on an edge that view hides and does not draw: \
                         draw the view with its hidden edges (style HiddenDashed) or show the dimension in a \
                         view that sees that edge",
                        placed_note.note,
                        note.text(),
                        placed_note.view
                    ));
                }
                // (A leader that runs across another view: it reads as
                // that view's.)
                if let Some((a, b)) = drawn.leader {
                    for other in frames.iter().filter(|o| o.view.id != placed.view.id) {
                        let (lo, hi) = other.bounds();
                        let inner = [lo + Point2::splat(1.0), hi - Point2::splat(1.0)];
                        if inner[0].cmplt(inner[1]).all() && crosses(a, b, &inner) {
                            laid.problems.push(format!(
                                "the leader of {} ({}) in the view {} runs across the view {}: put its \
                                 text on its own view's side",
                                placed_note.note,
                                note.text(),
                                placed_note.view,
                                other.view.id
                            ));
                        }
                    }
                }
                texts.push((drawn.text, start..laid.strokes.len()));
                taken.entry(&placed.view.id).or_default().push(&drawn);
                on_sheet.push(&drawn);
            }
            None => laid.problems.push(format!(
                "the view {} does not see {} true",
                placed_note.view, placed_note.note
            )),
        }
    }
    // Two lengths on one line whose lines run into each other — a text
    // put out past the arrows of its own across those of the other —
    // read as one line with no value between their arrows.
    for (k, a) in runs.iter().enumerate() {
        for b in &runs[k + 1..] {
            let parallel = a.3.perp_dot(b.3).abs() < 1e-6;
            let offset = if a.3.dot(b.3) > 0.0 { b.4 } else { -b.4 };
            if a.0 != b.0 || !parallel {
                continue;
            }
            // Two lengths side by side over a common stretch nearer than
            // a line of lettering: the one's text is on the other's line
            // (ISO 129-1: stacked 7 mm apart).
            let apart = (a.4 - offset).abs();
            if apart > 0.3 {
                let flip = |(x, y): (f64, f64)| {
                    if a.3.dot(b.3) > 0.0 { (x, y) } else { (-y, -x) }
                };
                let arrows = flip(b.6);
                let common = a.6.1.min(arrows.1) - a.6.0.max(arrows.0);
                if apart < lettering() + OFF + 0.5 && common > 0.5 {
                    laid.problems.push(format!(
                        "the dimensions {} ({}) and {} ({}) in the view {} run side by side {apart:.1} mm apart: the \
                         one's text stands on the other's line (stack them 7 mm apart, the shorter inside)",
                        a.1, a.2, b.1, b.2, a.0
                    ));
                }
                continue;
            }
            let flip = |(x, y): (f64, f64)| {
                if a.3.dot(b.3) > 0.0 { (x, y) } else { (-y, -x) }
            };
            let (span, arrows) = (flip(b.5), flip(b.6));
            // (Either's line over the other's arrows.)
            let over =
                |s: (f64, f64), arrows: (f64, f64)| s.0 < arrows.1 - 0.5 && s.1 > arrows.0 + 0.5;
            if over(a.5, arrows) || over(span, a.6) {
                laid.problems.push(format!(
                    "the dimensions {} ({}) and {} ({}) in the view {} lie on one line and run into each \
                     other (a text put out past the arrows of the other): put each text beside its own \
                     arrows, on its outer side",
                    a.1, a.2, b.1, b.2, a.0
                ));
            }
        }
    }
    // Words: beside a view, or under the value of the dimension they
    // belong to, turned as it is.
    for (index, words) in sheet.words.iter().enumerate() {
        let (foot, along) = match &words.dimension {
            Some(name) => {
                let of = values.iter().find(|(note, _, _)| note == name).or_else(|| {
                    values.iter().find(|(note, _, _)| {
                        note.strip_prefix(name.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                    })
                });
                let own = of.map(|(_, _, k)| k.clone()).unwrap_or(0..0);
                let value = of.and_then(|(_, k, _)| {
                    // (Under its last line: the lowest of its texts
                    // that stand as its value does — a callout's
                    // head under its bore.)
                    let value = laid.texts.get(k.start)?;
                    let up = from_angle(value.angle).perp();
                    laid.texts[k.clone()]
                        .iter()
                        .filter(|t| {
                            (t.angle - value.angle).abs() < 1e-6 && !t.text.trim().is_empty()
                        })
                        .min_by(|a, b| a.at.dot(up).total_cmp(&b.at.dot(up)))
                });
                let Some(value) = value else {
                    laid.problems.push(format!(
                        "the words \"{}\" belong to the dimension {name}, which is on no view of the sheet",
                        words.text
                    ));
                    continue;
                };
                let along = from_angle(value.angle);
                let up = along.perp();
                // (Their lines: as wide as the widest, one under the
                // other.)
                let (w, deep) = {
                    let rows: Vec<&str> = words.text.lines().collect();
                    let wide = rows
                        .iter()
                        .map(|r| width(r, words.height))
                        .fold(0.0, f64::max);
                    let deep = words.height * (1.0 + 1.6 * (rows.len().max(1) - 1) as f64);
                    (wide, deep)
                };
                // (Where the value's line of text ends, along it, and its
                // top: all of the dimension's lettering on that line.)
                let corners = text_corners(value, 0.0);
                let line_of = |t: &super::Text| {
                    (t.angle - value.angle).abs() < 1e-6
                        && ((t.at - value.at).dot(up)).abs() < value.height
                };
                let reach: Vec<f64> = of
                    .map(|(_, k, _)| k.clone())
                    .into_iter()
                    .flatten()
                    .filter(|k| line_of(&laid.texts[*k]))
                    .flat_map(|k| text_corners(&laid.texts[k], 0.0))
                    .map(|c| (c - corners[0]).dot(along))
                    .collect();
                let begin = reach.iter().copied().fold(0.0, f64::min);
                let end = reach
                    .iter()
                    .copied()
                    .fold((corners[1] - corners[0]).dot(along), f64::max);
                let top = corners[3];
                let shift = Point2::from_array(words.at);
                // Where they may stand, in turn (their top's middle):
                // under the line the value stands on, its middle under
                // the value's; on the value's line after it; under it
                // from where the value starts on, or back to where it
                // ends.
                let under = (corners[0] + corners[1]) / 2.0 - up * OFF + shift;
                let row = under - along * (under - corners[0]).dot(along);
                let candidates = [
                    under,
                    row + along * (begin + 0.6 + w / 2.0),
                    row + along * (end - 0.6 - w / 2.0),
                    top + along * (end + 1.5 + w / 2.0) + shift,
                ];
                // (Clear of the dimension's own lines — its extension
                // lines, arrows, a leader — and within the sheet's
                // frame.)
                let size = Point2::from_array(laid.size);
                let clear = |foot: Point2| {
                    let centre = foot - up * (deep / 2.0);
                    let lines = laid.strokes[own.clone()].iter().all(|s| {
                        (0..=16).all(|k| {
                            let q = s.a.lerp(s.b, f64::from(k) / 16.0) - centre;
                            // (Off a line along them as far as its
                            // arrowheads reach out from it.)
                            q.dot(along).abs() > w / 2.0 + 0.3 || q.dot(up).abs() > deep / 2.0 + 1.2
                        })
                    });
                    let inside = [w / 2.0, -w / 2.0].iter().all(|x| {
                        [0.0, deep].iter().all(|y| {
                            let c = foot + along * *x - up * *y;
                            c.cmpge(Point2::new(FRAMED[0], FRAMED[1])).all()
                                && c.cmple(size - Point2::splat(FRAMED[1])).all()
                        })
                    });
                    lines && inside
                };
                // (Off what else is written, as well, where they can be.)
                let alone = |foot: Point2| {
                    let ends = [
                        foot - along * (w / 2.0),
                        foot + along * (w / 2.0),
                        foot - along * (w / 2.0) - up * deep,
                        foot + along * (w / 2.0) - up * deep,
                    ];
                    let lo = ends
                        .iter()
                        .fold(Point2::splat(f64::INFINITY), |m, p| m.min(*p));
                    let hi = ends
                        .iter()
                        .fold(Point2::splat(f64::NEG_INFINITY), |m, p| m.max(*p));
                    texts.iter().all(|(rect, _)| {
                        rect[1].x <= lo.x
                            || rect[0].x >= hi.x
                            || rect[1].y <= lo.y
                            || rect[0].y >= hi.y
                    })
                };
                let foot = candidates
                    .iter()
                    .copied()
                    .find(|c| clear(*c) && alone(*c))
                    .or_else(|| candidates.iter().copied().find(|c| clear(*c)));
                let foot = foot.unwrap_or_else(|| {
                    // (Else along under it as far as they must be.)
                    let step = 0.5;
                    (0..=((w + 2.0 * deep) / step) as i32)
                        .flat_map(|k| [k, -k])
                        .map(|k| under + along * (f64::from(k) * step))
                        .find(|c| clear(*c))
                        .unwrap_or(under)
                });
                (foot, along)
            }
            None => {
                let from = match &words.view {
                    Some(id) => match frames.iter().find(|placed| placed.view.id == *id) {
                        Some(placed) => placed.centre,
                        None => continue,
                    },
                    None => Point2::ZERO,
                };
                // (Their middle where they are put.)
                let middle = from + Point2::from_array(words.at);
                (middle + Point2::Y * (words.height / 2.0), Point2::X)
            }
        };
        // (Several lines, one under the other, left-aligned.)
        let rows: Vec<&str> = words.text.lines().collect();
        let rows = if rows.is_empty() { vec![""] } else { rows };
        let wide = rows
            .iter()
            .map(|r| width(r, words.height))
            .fold(0.0, f64::max);
        let up = along.perp();
        let pitch = words.height * 1.6;
        for (k, row) in rows.iter().enumerate() {
            // (One line by its middle, as it always stood.)
            let (at, anchor) = match rows.len() {
                1 => (foot, [0.5, 1.0]),
                _ => (
                    foot - along * (wide / 2.0) - up * (pitch * k as f64),
                    [0.0, 1.0],
                ),
            };
            laid.texts.push(super::Text {
                at,
                text: (*row).to_owned(),
                height: words.height,
                anchor,
                angle: libm::atan2(along.y, along.x),
                preview: false,
            });
        }
        let half = along * (wide / 2.0);
        let deep = words.height + pitch * (rows.len() - 1) as f64;
        let corners = [
            foot - half,
            foot + half,
            foot - half - up * deep,
            foot + half - up * deep,
        ];
        let lo = corners
            .iter()
            .fold(Point2::splat(f64::INFINITY), |m, p| m.min(*p));
        let hi = corners
            .iter()
            .fold(Point2::splat(f64::NEG_INFINITY), |m, p| m.max(*p));
        let at = laid.strokes.len();
        // Their leader, to what they say something of.
        let view = words
            .view
            .as_ref()
            .and_then(|id| frames.iter().find(|p| p.view.id == *id));
        // (One from the words to each place they say something of.)
        let mut tips = Vec::new();
        if let Some(placed) = view {
            let lines = drawn_lines(placed);
            for to in &words.to {
                let tip = match *to {
                    crate::sheet::Target::Model(p) => placed.place(Point3::from_array(p)),
                    crate::sheet::Target::View(p) => placed.sheet(Point2::from_array(p)),
                };
                words_leader(laid, [lo, hi], tip, &lines);
                tips.push(tip);
            }
        }
        texts.push(([lo, hi], at..laid.strokes.len()));
        on_sheet.push(&Drawn::of([lo, hi]));
        laid.notes.push(LaidNote {
            index,
            kind: super::Placing::Words,
            rect: [lo, hi],
            view: words.view.clone().unwrap_or_default(),
            preview: None,
            linear: None,
            path: Vec::new(),
            tips,
        });
    }
    for (index, own) in sheet.own.iter().enumerate() {
        let Some(placed) = frames.iter().find(|p| p.view.id == own.view) else {
            continue;
        };
        let (from, to) = (Point2::from_array(own.from), Point2::from_array(own.to));
        let (p, q) = (placed.sheet(from), placed.sheet(to));
        let at = placed.centre + Point2::from_array(own.at);
        let start = laid.strokes.len();
        let drawn = if own.round {
            // Something round, by its middle and a point of it: its
            // diameter, on a line through the middle.
            let r = p.distance(q);
            let way = (at - p).try_normalize().unwrap_or(Point2::X);
            let reach = (at - p).dot(way).max(r);
            laid.line(p - way * r, p + way * reach, Pen::Narrow);
            laid.arrow(p + way * r, way, ARROW);
            laid.arrow(p - way * r, -way, ARROW);
            let text = said(from.distance(to) * 2.0, "Ø", "", &Tolerance::General);
            Drawn::of(dimension_text(laid, at, way, &text))
        } else {
            let (along, value) = match own.upright {
                Some(false) => (Point2::X, (to.x - from.x).abs()),
                Some(true) => (Point2::Y, (to.y - from.y).abs()),
                None => ((q - p).normalize_or_zero(), from.distance(to)),
            };
            if along == Point2::ZERO {
                continue;
            }
            let text = said(value, "", "", &Tolerance::General);
            let gap = sheet.spacing.gap;
            linear(laid, (p, q), along, at, &text, [gap; 2], None)
        };
        laid.notes.push(LaidNote {
            index,
            kind: super::Placing::Own,
            rect: drawn.text,
            view: placed.view.id.clone(),
            preview: None,
            linear: drawn.linear,
            path: Vec::new(),
            tips: Vec::new(),
        });
        texts.push((drawn.text, start..laid.strokes.len()));
        taken.entry(&placed.view.id).or_default().push(&drawn);
        on_sheet.push(&drawn);
    }
    // What is said of the model and stands in no view: counted, and
    // (asked for) drawn as what it would be.
    for (name, note) in notes {
        if sheet.notes.iter().any(|n| n.note == *name) {
            continue;
        }
        laid.missing.push(name.clone());
        let Some(only) = previews else {
            continue;
        };
        for placed in frames {
            // (Asked for in one view, it is shown there if that view
            // sees it true, a detail too.)
            let shows = match only {
                Some(_) => sees(note, placed),
                None => fits(note, placed),
            };
            if only.is_some_and(|id| id != placed.view.id) || !shows {
                continue;
            }
            let there = taken.entry(&placed.view.id).or_default();
            let at = place_by_itself(note, placed, there);
            let (strokes, lettering, fills) =
                (laid.strokes.len(), laid.texts.len(), laid.fills.len());
            let gap = [sheet.spacing.gap; 2];
            let Some(drawn) = draw_note(
                laid,
                note,
                placed,
                placed.centre + at,
                gap,
                &Attach::default(),
            ) else {
                continue;
            };
            for stroke in &mut laid.strokes[strokes..] {
                stroke.preview = true;
            }
            for text in &mut laid.texts[lettering..] {
                text.preview = true;
            }
            for fill in &mut laid.fills[fills..] {
                fill.colour = super::PREVIEW;
            }
            laid.notes.push(LaidNote {
                index: 0,
                kind: super::Placing::Note,
                rect: drawn.text,
                view: placed.view.id.clone(),
                preview: Some((name.clone(), at.to_array())),
                linear: drawn.linear,
                path: Vec::new(),
                tips: Vec::new(),
            });
            texts.push((drawn.text, strokes..laid.strokes.len()));
            there.push(&drawn);
        }
    }
    clear(laid, &texts);
    // (What clearing does not break: an outline through a text.)
    let mut said = Vec::new();
    for n in laid
        .notes
        .iter()
        .filter(|n| n.kind == super::Placing::Note && n.preview.is_none())
    {
        let shrunk = [
            n.rect[0] + Point2::splat(0.5),
            n.rect[1] - Point2::splat(0.5),
        ];
        if shrunk[0].cmpge(shrunk[1]).any() {
            continue;
        }
        let through = laid
            .strokes
            .iter()
            .any(|s| s.pen == Pen::Wide && !s.preview && crosses(s.a, s.b, &shrunk));
        if let (true, Some(placed)) = (through, sheet.notes.get(n.index)) {
            said.push(format!(
                "the text of {} in the view {} stands on an outline: put it off the part",
                placed.note, placed.view
            ));
        }
    }
    laid.problems.extend(said);
    on_sheet
}

/// Of the view `placed`, whether it is the one to show `note` in by
/// itself: one that sees it true, and of a datum or a tolerance, one
/// that sees its face edge on (`edge_on`).
fn suits(note: &Annotation, placed: &Placed<'_>, edge_on: bool) -> bool {
    fits(note, placed)
        && (!edge_on
            || match &note.note {
                Note::Datum { out, .. }
                | Note::Geometric { out, .. }
                | Note::Texture { out, .. } => placed.way(*out).length() > 0.5,
                // (A hole where it is seen as a circle; a rod's thread
                // from the side.)
                Note::Hole { axis, outside, .. } => square_on(*axis, placed.towards()) != *outside,
                Note::Dimension { .. } => true,
            })
}

/// Where each of `wanted` stands on `sheet`, one after the other, each
/// clear of what stands on the sheet already — in any view — of those
/// put before it, and of the views it is not in: the view (the one
/// asked for, if that sees it true; else the first that does), where
/// its text stands from that view's middle (where it is asked to, if
/// it is), and whether that is the view asked for. Nothing for what no
/// view sees, or what is said of the model no longer.
pub(crate) fn place_all(
    sheet: &Sheet,
    subject: &Subject<'_>,
    frames: &[Placed<'_>],
    wanted: &[super::Wanted],
) -> Vec<Option<(String, [f64; 2], bool)>> {
    let mut scratch = Laid::default();
    // (What the views write of their own — their names, the letters of
    // their cuts and details — is in the way as well.)
    for placed in frames {
        super::views::draw(&mut scratch, sheet, placed, frames);
    }
    let lettering: Vec<[Point2; 2]> = scratch.texts.iter().map(text_box).collect();
    let mut taken = draw(&mut scratch, sheet, subject, frames, None);
    taken.boxes.extend(lettering);
    // (As far in as the build brings in what stands out past the frame.)
    let [w, h] = sheet.size();
    taken.within = Some([Point2::splat(12.5), Point2::new(w - 12.5, h - 12.5)]);
    let gap = [sheet.spacing.gap; 2];
    let view_of = |want: &super::Wanted| -> Option<(&Placed<'_>, bool)> {
        let note = subject.world.said(&want.note)?;
        let note = &*note;
        let asked = want
            .view
            .as_deref()
            .and_then(|id| frames.iter().find(|p| p.view.id == id && sees(note, *p)));
        if let Some(asked) = asked {
            return Some((asked, true));
        }
        let first = frames
            .iter()
            .find(|p| suits(note, p, true))
            .or_else(|| frames.iter().find(|p| suits(note, p, false)))?;
        Some((first, want.view.is_none()))
    };
    let mut out: Vec<Option<(String, [f64; 2], bool)>> = vec![None; wanted.len()];
    // Those whose place is said first, then the others in turn — of
    // the lengths among them the shorter first, nearer the outline, so
    // that a longer one stands outside them (ISO 129-1).
    let length = |k: usize| match subject.world.said(&wanted[k].note).as_deref() {
        Some(Annotation {
            note:
                Note::Dimension {
                    measured: Measured::Length { from, to, along },
                    ..
                },
            ..
        }) => Some(along.map_or(from.distance(*to), |a| (*to - *from).dot(a).abs())),
        _ => None,
    };
    let free: Vec<usize> = (0..wanted.len())
        .filter(|k| wanted[*k].at.is_none())
        .collect();
    let mut lengths: Vec<(usize, f64)> = free
        .iter()
        .filter_map(|k| length(*k).map(|l| (*k, l)))
        .collect();
    let slots: Vec<usize> = lengths.iter().map(|(k, _)| *k).collect();
    lengths.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut sorted = lengths.into_iter().map(|(k, _)| k);
    let free: Vec<usize> = free
        .into_iter()
        .map(|k| match slots.contains(&k) {
            true => sorted.next().unwrap_or(k),
            false => k,
        })
        .collect();
    let order = (0..wanted.len())
        .filter(|k| wanted[*k].at.is_some())
        .chain(free);
    for k in order {
        let want = &wanted[k];
        let (Some(note), Some((placed, as_asked))) =
            (subject.world.said(&want.note), view_of(want))
        else {
            continue;
        };
        let note = &*note;
        let at = match want.at {
            Some(at) => Point2::from_array(at),
            None => {
                let mut here = taken.clone();
                here.views = frames
                    .iter()
                    .filter(|other| other.view.id != placed.view.id)
                    .map(|other| {
                        let (lo, hi) = other.bounds();
                        [lo, hi]
                    })
                    .collect();
                place_by_itself(note, placed, &here)
            }
        };
        if let Some(drawn) = draw_note(
            &mut scratch,
            note,
            placed,
            placed.centre + at,
            gap,
            &Attach::default(),
        ) {
            taken.push(&drawn);
        }
        out[k] = Some((placed.view.id.clone(), at.to_array(), as_asked));
    }
    out
}

/// Where the text of `note` stands by itself in the view `placed`, in
/// millimetres from the view's middle: off what it is about, outside
/// the view where it can be (ISO 129-1: the first dimension line 10
/// from the outline, the next 7 on), and clear of what is `taken`
/// there already: going on outwards while it is in the way, a length
/// on its other side where it would run onto another view first; a
/// radius turned about its round before it goes farther out.
pub(crate) fn place_by_itself(note: &Annotation, placed: &Placed<'_>, taken: &Taken) -> Point2 {
    // (Measured where this view sees it true.)
    let seen = as_seen(note, placed, None);
    let note = seen.as_deref().unwrap_or(note);
    let (lo, hi) = placed.bounds();
    let anchors: Vec<Point2> = note.anchors().iter().map(|p| placed.place(*p)).collect();
    let about = anchors.iter().sum::<Point2>() / anchors.len().max(1) as f64;
    let middle = (lo + hi) / 2.0;
    // Where it starts, and the way it goes on from there while it is
    // in the way of something: one way or more, tried in turn.
    let rays: Vec<(Point2, Point2)> = match &note.note {
        Note::Dimension {
            measured: Measured::Length { from, to, along },
            ..
        } => {
            // Beside the view, on the side it is nearer to, square to
            // what it measures; else on the other side.
            let ((p, q), along) =
                length_ends(placed, *from, *to, *along).unwrap_or(((about, about), Point2::X));
            let about = (p + q) / 2.0;
            let across = along.perp();
            let near_side = if (about - middle).dot(across) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            [near_side, -near_side]
                .into_iter()
                .map(|side| {
                    let reach = [lo, hi, Point2::new(lo.x, hi.y), Point2::new(hi.x, lo.y)]
                        .iter()
                        .map(|c| (*c - about).dot(across) * side)
                        .fold(0.0_f64, f64::max);
                    (
                        about + across * (side * (reach + 10.0)),
                        across * (side * 7.0),
                    )
                })
                .collect()
        }
        Note::Dimension {
            measured:
                Measured::Diameter {
                    centre,
                    axis,
                    radius,
                }
                | Measured::Radius {
                    centre,
                    axis,
                    radius,
                }
                | Measured::Radial {
                    centre,
                    axis,
                    outer: radius,
                    ..
                },
            ..
        } if !square_on(*axis, placed.towards()) => {
            // Seen from the side it is drawn as a length across the
            // axis: beside the view along the axis, as a length is; a
            // radius (and a wall) over its own half, the side away
            // from the view's middle.
            let c = placed.place(*centre);
            let up = placed.way(*axis).normalize_or_zero();
            let across = up.perp();
            let half = match &note.note {
                Note::Dimension {
                    measured: Measured::Diameter { .. },
                    ..
                } => 0.0,
                Note::Dimension {
                    measured: Measured::Radial { inner, .. },
                    ..
                } => (inner + radius) / 2.0,
                _ => radius / 2.0,
            };
            let lateral = if (c - middle).dot(across) >= 0.0 {
                1.0
            } else {
                -1.0
            };
            let side = if (c - middle).dot(up) >= -1e-6 {
                1.0
            } else {
                -1.0
            };
            let reach = [lo, hi, Point2::new(lo.x, hi.y), Point2::new(hi.x, lo.y)]
                .iter()
                .map(|corner| (*corner - c).dot(up) * side)
                .fold(0.0_f64, f64::max);
            vec![(
                c + across * (lateral * half * placed.scale) + up * (side * (reach + 10.0)),
                up * (side * 7.0),
            )]
        }
        Note::Dimension {
            measured:
                Measured::Diameter { radius, .. }
                | Measured::Radius { radius, .. }
                | Measured::Radial { outer: radius, .. },
            ..
        } => {
            let way = (about - middle)
                .try_normalize()
                .unwrap_or(Point2::new(1.0, 1.0).normalize());
            // (Not along a centre line.)
            let way = if way.x.abs() < 0.2 || way.y.abs() < 0.2 {
                Point2::new(
                    if way.x < 0.0 { -1.0 } else { 1.0 },
                    if way.y < 0.0 { -1.0 } else { 1.0 },
                )
                .normalize()
            } else {
                way
            };
            // (Turned about the round, a little and more, either way,
            // before going farther out: a radius beside another of the
            // same middle has a leader of its own.)
            [0.0_f64, 20.0, -20.0, 40.0, -40.0, 60.0, -60.0]
                .into_iter()
                .map(|turn| {
                    let (sin, cos) = libm::sincos(turn.to_radians());
                    let way = Point2::new(way.x * cos - way.y * sin, way.x * sin + way.y * cos);
                    (about + way * (radius * placed.scale + 12.0), way * 8.0)
                })
                .collect()
        }
        Note::Dimension {
            measured: Measured::Angle { first, second, .. },
            ..
        } => {
            let between = (placed.way(*first).normalize_or_zero()
                + placed.way(*second).normalize_or_zero())
            .try_normalize()
            .unwrap_or(Point2::X);
            vec![(about + between * 20.0, between * 8.0)]
        }
        Note::Hole { radius, .. } => {
            // (Off the hole slantwise, out from the view's middle.)
            let way = (about - middle)
                .try_normalize()
                .unwrap_or(Point2::new(1.0, 1.0).normalize());
            let way = Point2::new(
                if way.x < 0.0 { -1.0 } else { 1.0 },
                if way.y < 0.0 { -1.0 } else { 1.0 },
            )
            .normalize();
            // (Turned farther round, to the view's other sides, where
            // those are in the way: its leader keeps off other views.)
            [0.0_f64, 20.0, -20.0, 40.0, -40.0, 90.0, -90.0, 180.0]
                .into_iter()
                .map(|turn| {
                    let (sin, cos) = libm::sincos(turn.to_radians());
                    let way = Point2::new(way.x * cos - way.y * sin, way.x * sin + way.y * cos);
                    // (Out of the view, as far as it reaches that way.)
                    let reach = [lo, hi, Point2::new(lo.x, hi.y), Point2::new(hi.x, lo.y)]
                        .iter()
                        .map(|c| (*c - about).dot(way))
                        .fold(radius * placed.scale, f64::max);
                    (about + way * (reach + 8.0), way * 8.0)
                })
                .collect()
        }
        // (Of an axis the view looks along: off its circle slantwise, out
        // from the view's middle, as a hole's callout.)
        Note::Datum { axis: Some(a), .. } | Note::Geometric { axis: Some(a), .. }
            if square_on(a.axis, placed.towards()) =>
        {
            let c = placed.place(a.centre);
            let way = (c - middle)
                .try_normalize()
                .unwrap_or(Point2::new(1.0, 1.0).normalize());
            let way = Point2::new(
                if way.x < 0.0 { -1.0 } else { 1.0 },
                if way.y < 0.0 { -1.0 } else { 1.0 },
            )
            .normalize();
            [0.0_f64, 20.0, -20.0, 40.0, -40.0, 90.0, -90.0, 180.0]
                .into_iter()
                .map(|turn| {
                    let (sin, cos) = libm::sincos(turn.to_radians());
                    let way = Point2::new(way.x * cos - way.y * sin, way.x * sin + way.y * cos);
                    let reach = [lo, hi, Point2::new(lo.x, hi.y), Point2::new(hi.x, lo.y)]
                        .iter()
                        .map(|q| (*q - c).dot(way))
                        .fold(a.radius * placed.scale, f64::max);
                    (c + way * (reach + 8.0), way * 8.0)
                })
                .collect()
        }
        Note::Datum { out, .. } | Note::Geometric { out, .. } | Note::Texture { out, .. } => {
            let out = placed.way(*out);
            if out.length() > 0.5 {
                let out = out.normalize();
                vec![(about + out * 18.0, out.perp() * 10.0)]
            } else {
                vec![(Point2::new(hi.x + 22.0, about.y + 12.0), Point2::Y * 10.0)]
            }
        }
    };
    let start = rays.first().map_or(about, |r| r.0);
    // (Rays turned about a round go out a few steps each, in turn;
    // others each as far as it takes.)
    let (steps, rounds) = if rays.len() > 2 {
        (5, true)
    } else {
        (40, false)
    };
    let mut tries: Vec<(usize, Point2)> = Vec::new();
    if rounds {
        for k in 0..steps {
            for (ray, (from, step)) in rays.iter().enumerate() {
                tries.push((ray, *from + *step * k as f64));
            }
        }
    } else {
        for (ray, (from, step)) in rays.iter().enumerate() {
            tries.extend((0..steps).map(|k| (ray, *from + *step * k as f64)));
        }
    }
    let mut given_up = vec![false; rays.len()];
    // (Its text keeps off the view's own lines: a stack of limits is
    // higher than one line, a radius's text not on the round.)
    let own_lines = drawn_lines(placed);
    let mut fallback: Option<(usize, Point2)> = None;
    for (ray, at) in tries {
        if given_up[ray] {
            continue;
        }
        let mut scratch = Laid::default();
        let Some(drawn) = draw_note(&mut scratch, note, placed, at, [0.0; 2], &Attach::default())
        else {
            break;
        };
        // (Not over another view, nor past it.)
        if taken.on_a_view(&drawn) {
            given_up[ray] = true;
            continue;
        }
        let shrunk = [
            drawn.text[0] + Point2::splat(0.2),
            drawn.text[1] - Point2::splat(0.2),
        ];
        if own_lines.iter().any(|(a, b)| crosses(*a, *b, &shrunk)) {
            continue;
        }
        if !taken.blocks(&drawn) {
            // (Of those clear, the first whose leader runs across
            // nothing the view draws — not over the part, nor over a
            // section's hatching — where there is one: outside.)
            let across = drawn
                .leader
                .map_or(0, |(a, b)| leader_across(placed, &own_lines, a, b));
            if across == 0 {
                return at - placed.centre;
            }
            // (Else of those, the one that runs across least.)
            if fallback.is_none_or(|(least, _)| across < least) {
                fallback = Some((across, at));
            }
        }
    }
    fallback.map_or(start, |(_, at)| at) - placed.centre
}

/// How much a leader from `a` to `b` (on the sheet) runs across what
/// the view `placed` draws: how many of `lines` (its lines) it crosses,
/// and in how many eighths of its length it lies over what its section
/// cuts (its hatching) — but at its ends, where it meets what it points
/// at.
fn leader_across(placed: &Placed<'_>, lines: &[(Point2, Point2)], a: Point2, b: Point2) -> usize {
    let Some(way) = (b - a).try_normalize() else {
        return 0;
    };
    if a.distance(b) < 2.5 {
        return 0;
    }
    let (a, b) = (a + way * 1.0, b - way * 1.0);
    let crosses = |p: Point2, q: Point2| {
        let (r, s) = (b - a, q - p);
        let den = r.perp_dot(s);
        if den.abs() < 1e-12 {
            return false;
        }
        let t = (p - a).perp_dot(s) / den;
        let u = (p - a).perp_dot(r) / den;
        (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
    };
    let crossed = lines.iter().filter(|(p, q)| crosses(*p, *q)).count();
    let inside = |p: Point2, [x, y, z]: [Point2; 3]| {
        let d = |u: Point2, v: Point2| (v - u).perp_dot(p - u);
        let (e, f, g) = (d(x, y), d(y, z), d(z, x));
        (e >= 0.0 && f >= 0.0 && g >= 0.0) || (e <= 0.0 && f <= 0.0 && g <= 0.0)
    };
    let hatched = (1..8)
        .filter(|k| {
            let p = a.lerp(b, f64::from(*k) / 8.0);
            placed
                .seen
                .cut
                .iter()
                .any(|(t, _)| inside(p, t.map(|c| placed.sheet(c))))
        })
        .count();
    crossed + hatched
}

/// What is said on the model itself, drawn in the plane it lies in: the
/// lines, the arrowheads and the lettering of a note as a drawing has
/// them (ISO 129-1, ISO 1101, ISO 5459; on a model, ISO 16792).
pub mod on_model {
    use super::{Frame, Laid, draw_note, width};
    use workpiece_core::math::{Point2, Point3, from_angle};
    use workpiece_core::world::{Annotation, Measured, Note};

    /// A line of lettering on the model.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Lettering {
        /// Its middle.
        pub at: Point3,
        /// The way it is read along (a unit vector).
        pub along: Point3,
        /// What it says.
        pub text: String,
        /// How high its capitals are, in the model's lengths.
        pub height: f64,
    }

    /// A note as it is drawn on the model.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct OnModel {
        /// Its lines.
        pub strokes: Vec<(Point3, Point3)>,
        /// Its arrowheads and other filled areas.
        pub fills: Vec<[Point3; 3]>,
        /// Its lettering.
        pub texts: Vec<Lettering>,
        /// The corners of the box of its text, around it: what is
        /// pointed at and dragged.
        pub grip: [Point3; 4],
    }

    struct Plane {
        at: Point3,
        x: Point3,
        y: Point3,
        per_mm: f64,
    }

    impl Frame for Plane {
        fn place(&self, p: Point3) -> Point2 {
            let d = p - self.at;
            Point2::new(d.dot(self.x), d.dot(self.y)) / self.per_mm
        }

        fn way(&self, d: Point3) -> Point2 {
            Point2::new(d.dot(self.x), d.dot(self.y))
        }

        fn scale(&self) -> f64 {
            1.0 / self.per_mm
        }

        fn towards(&self) -> Point3 {
            self.x.cross(self.y)
        }
    }

    /// `v` without what of it is along `n`.
    fn without(v: Point3, n: Point3) -> Point3 {
        v - n * v.dot(n)
    }

    /// The plane `note` lies in with its text at `at`: square to this.
    /// A length lies in the plane of what it measures and where its
    /// text stands (a sketch's, in the sketch); something round, square
    /// to its axis; a datum or a tolerance faces who looks, so that its
    /// frame is read whichever way the model is turned.
    fn normal(note: &Annotation, at: Point3, right: Point3, up: Point3) -> Point3 {
        // Through `along` and, as far as can be, where the text is. Where
        // the text is on `along` itself (or so near it that which way
        // it lies off is rounding), the plane is the one through the
        // model's axis that `along` is least along: fixed by the model,
        // so that turning the view never turns the plane.
        let through = |along: Point3, from: Point3| {
            let off = without(at - from, along);
            let scale = (at - from).length().max(1e-300);
            let off = if off.length() > 1e-6 * scale {
                off
            } else {
                let axis = [Point3::Z, Point3::Y, Point3::X]
                    .into_iter()
                    .min_by(|a, b| a.dot(along).abs().total_cmp(&b.dot(along).abs()))
                    .unwrap_or(Point3::Z);
                without(axis, along)
            };
            along.cross(off).normalize_or_zero()
        };
        match &note.note {
            Note::Dimension { measured, .. } => match measured {
                Measured::Length { from, to, along } => {
                    through(along.unwrap_or(*to - *from).normalize_or_zero(), *from)
                }
                Measured::Diameter { axis, .. }
                | Measured::Radius { axis, .. }
                | Measured::Radial { axis, .. } => *axis,
                Measured::Angle { first, second, .. } => first.cross(*second).normalize_or_zero(),
            },
            Note::Datum { .. } | Note::Geometric { .. } | Note::Texture { .. } => {
                right.cross(up).normalize_or_zero()
            }
            Note::Hole { axis, .. } => *axis,
        }
    }

    /// `note` with its text at `at`, as it is drawn on the model for
    /// who sees `right` to the right and `up` up, one millimetre of a
    /// drawing's lettering being `per_mm` of the model's lengths.
    pub fn on_model(
        note: &Annotation,
        at: Point3,
        right: Point3,
        up: Point3,
        per_mm: f64,
    ) -> Option<OnModel> {
        let n = normal(note, at, right, up);
        if n == Point3::ZERO || per_mm <= 0.0 {
            return None;
        }
        // (A frame's plane goes through the face it points at: its
        // leader ends on the face.)
        let at = match &note.note {
            Note::Datum { on, .. } | Note::Geometric { on, .. } | Note::Texture { on, .. } => {
                at + n * (*on - at).dot(n)
            }
            Note::Dimension { .. } | Note::Hole { .. } => at,
        };
        // Read from where it is looked at: across as the eye's right is,
        // up as its up is, as nearly as the plane has it. Of the two it
        // is the one that keeps the more of its length in the plane that
        // gives the frame (the other is square to it): a plane seen
        // edge-on keeps none of one of them, and nothing of a vector
        // that short is a direction.
        let (r, u) = (without(right, n), without(up, n));
        let (x, y) = if r.length() >= u.length() {
            let x = r.try_normalize()?;
            let y = n.cross(x);
            (x, if y.dot(up) < 0.0 { -y } else { y })
        } else {
            let y = u.try_normalize()?;
            let x = y.cross(n);
            (if x.dot(right) < 0.0 { -x } else { x }, y)
        };
        let plane = Plane { at, x, y, per_mm };
        let mut laid = Laid::default();
        let drawn = draw_note(
            &mut laid,
            note,
            &plane,
            Point2::ZERO,
            [0.0; 2],
            &super::Attach::default(),
        )?;
        let back = |p: Point2| at + (x * p.x + y * p.y) * per_mm;
        let [lo, hi] = drawn.text;
        Some(OnModel {
            strokes: laid
                .strokes
                .iter()
                .map(|s| (back(s.a), back(s.b)))
                .collect(),
            fills: laid.fills.iter().map(|f| f.corners.map(back)).collect(),
            texts: laid
                .texts
                .iter()
                .map(|t| {
                    let along = from_angle(t.angle);
                    let size = Point2::new(width(&t.text, t.height), t.height);
                    let off = (Point2::new(0.5, 0.5) - Point2::from_array(t.anchor)) * size;
                    let middle = t.at + along * off.x + along.perp() * off.y;
                    Lettering {
                        at: back(middle),
                        along: (x * along.x + y * along.y).normalize_or_zero(),
                        text: t.text.clone(),
                        height: t.height * per_mm,
                    }
                })
                .collect(),
            grip: [
                back(lo),
                back(Point2::new(hi.x, lo.y)),
                back(hi),
                back(Point2::new(lo.x, hi.y)),
            ],
        })
    }
}
