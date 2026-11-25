//! What the pointer picks on a sheet, as in the 3D view: every
//! candidate under it in one ordered list — what is grabbed by a handle
//! first, then what stands in a view, what is missing (while that
//! shows), the cutting lines and detail circles, the views, the title
//! block. Hovering shows the current one with its name and "i/n"; the
//! right button goes on to the next; a left click confirms it (a press
//! drags it).

use shapecraft_core::math::Point2;
use shapecraft_ops_drawing::Sheet;
use shapecraft_ops_drawing::lay::{Laid, LaidNote, LaidView, Placing};
use shapecraft_ops_drawing::sheet::ViewKind;

/// Something on a sheet the pointer can pick.
#[derive(Debug, Clone, PartialEq)]
pub enum SheetPick {
    /// The text of a placed note — a dimension's value, a balloon:
    /// dragged, it moves the note.
    Text {
        /// Its place among the sheet's of its kind.
        index: usize,
        /// What it is.
        kind: Placing,
    },
    /// Where the extension line `k` of the sheet's note `index` starts.
    Start {
        /// The note.
        index: usize,
        /// The extension line: 0 or 1.
        k: usize,
    },
    /// The corner `index` of the cutting line of the section `view`.
    Corner {
        /// The section view.
        view: String,
        /// The corner.
        index: usize,
    },
    /// An arrow of the cutting line of the section `view`, at the end of
    /// the line that has the corner `index`: dragged, it moves that
    /// corner (and the arrow with it).
    Arrow {
        /// The section view.
        view: String,
        /// The corner it moves.
        index: usize,
    },
    /// Where the leader `k` of the sheet's words `index` ends.
    Tip {
        /// The words.
        index: usize,
        /// The leader.
        k: usize,
    },
    /// The end `k` of the sheet's (centre or datum) line `index`.
    End {
        /// The line.
        index: usize,
        /// The end: 0 or 1.
        k: usize,
    },
    /// The middle of what the detail `view` shows, in the view it is of.
    Middle {
        /// The detail view.
        view: String,
    },
    /// The handle on the circle of the detail `view`: how far around
    /// its middle it shows.
    Rim {
        /// The detail view.
        view: String,
    },
    /// The caption of `view` (`B–B`, `A (5:1)`).
    Caption {
        /// The view.
        view: String,
    },
    /// A placed note as a whole — a dimension by its lines, words, a
    /// centre or datum line.
    Note {
        /// Its place among the sheet's of its kind.
        index: usize,
        /// What it is.
        kind: Placing,
    },
    /// Something missing from the sheet, shown as what it would be: the
    /// `index`-th of the laid out notes. Confirmed, it is put there.
    Missing {
        /// Its place among [`Laid::notes`].
        index: usize,
    },
    /// The cutting line of the section `view`, in the view it shows in.
    CutLine {
        /// The section view.
        view: String,
    },
    /// The circle of the detail `view`, in the view it is of.
    Circle {
        /// The detail view.
        view: String,
    },
    /// A view.
    View {
        /// The view.
        view: String,
    },
    /// The cell `index` of the title block.
    Cell {
        /// The cell.
        index: usize,
    },
    /// A point of a view (an end of its lines, a middle of something
    /// round): what a dimension of the sheet's own is put on.
    Point {
        /// The view.
        view: String,
        /// Where it is, on the sheet.
        at: Point2,
    },
    /// Something round seen square on in a view.
    Round {
        /// The view.
        view: String,
        /// Its middle, on the sheet.
        centre: Point2,
        /// Its radius, on the sheet.
        radius: f64,
    },
    /// A line of a view.
    Edge {
        /// The view.
        view: String,
        /// Its ends, on the sheet.
        ends: [Point2; 2],
    },
}

/// What is looked for under the pointer: what the tool in hand works
/// on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickMode<'a> {
    /// Selecting and dragging; with `missing`, what is missing shows
    /// and is picked to be put on the sheet.
    Select {
        /// Whether what is missing shows.
        missing: bool,
    },
    /// The views (not the details): what a section's line or a detail
    /// is drawn in.
    Views,
    /// What a dimension of the sheet's own is of: points, rounds and
    /// lines of the views (of the view `only`, if it is said).
    Measure {
        /// The view picked from so far.
        only: Option<&'a str>,
    },
}

/// What [`candidates`] looks at.
pub struct Sheeted<'a> {
    /// The sheet.
    pub sheet: &'a Sheet,
    /// It laid out.
    pub laid: &'a Laid,
    /// The section views whose cutting line can be dragged.
    pub movable: &'a [String],
}

/// How far `p` is from the line from `a` to `b`.
fn off_line(p: Point2, a: Point2, b: Point2) -> f64 {
    let d = b - a;
    let t = if d.length_squared() > 0.0 {
        ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + d * t).distance(p)
}

/// How far `p` is outside the box `rect` (grown by `pad`): 0 inside.
fn off_box(p: Point2, rect: [Point2; 2], pad: f64) -> f64 {
    let lo = rect[0] - Point2::splat(pad);
    let hi = rect[1] + Point2::splat(pad);
    (lo - p).max(p - hi).max(Point2::ZERO).length()
}

/// The note `n` of `laid` as it stands on the sheet (not missing).
fn placed(n: &LaidNote) -> bool {
    n.preview.is_none()
}

/// Where the extension line `k` of the laid note `n` (one of the
/// sheet's notes) starts, on the sheet.
pub fn start_of(sheet: &Sheet, n: &LaidNote, k: usize) -> Option<Point2> {
    let (Placing::Note, Some(linear)) = (&n.kind, n.linear) else {
        return None;
    };
    let note = sheet.notes.get(n.index)?;
    let start = note.starts[k].unwrap_or(sheet.spacing.gap);
    let end = linear.from[k].distance(linear.line[k]);
    Some(linear.from[k] + linear.ways[k] * start.min(end))
}

/// The middle of the detail `view` and the handle on its circle, on the
/// sheet, in the view it is of; and its circle's radius there.
pub fn detail_handles(laid: &Laid, sheet: &Sheet, view: &str) -> Option<(Point2, Point2, f64)> {
    let ViewKind::Detail {
        of, centre, radius, ..
    } = &sheet.view(view)?.kind
    else {
        return None;
    };
    let parent = laid.views.iter().find(|v| v.id == *of)?;
    let middle = parent.anchor + (Point2::from_array(*centre) - parent.origin) * parent.scale;
    let r = radius * parent.scale;
    let rim = middle - Point2::new(1.0, 1.0).normalize() * r;
    Some((middle, rim, r))
}

/// Of the cutting line `cut`, the corner an arrow at `at` moves: the
/// corner nearest it.
fn arrow_corner(path: &[Point2], at: Point2) -> usize {
    path.iter()
        .enumerate()
        .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
        .map_or(0, |(k, _)| k)
}

/// Everything under the pointer at `p` (on the sheet) that `mode`
/// picks, `reach` millimetres near, the one to pick first first: the
/// handles that drag a part of something (a dimension's text and the
/// ends of its extension lines, the corners and arrows of a cutting
/// line, a leader's end, a balloon, a detail's middle and circle, a
/// caption), then what stands in a view (dimensions, words, centre and
/// datum lines), then what is missing (while that shows), the cutting
/// lines and detail circles, the views, the title block. Of each kind,
/// the nearest first.
pub fn candidates(of: &Sheeted<'_>, mode: PickMode<'_>, p: Point2, reach: f64) -> Vec<SheetPick> {
    let Sheeted {
        sheet,
        laid,
        movable,
    } = *of;
    // (Each with its class and how far off it is.)
    let mut found: Vec<(u8, f64, SheetPick)> = Vec::new();
    let in_view = |v: &LaidView| {
        (p.x - v.centre.x).abs() <= v.half.x.max(3.0) + 2.0
            && (p.y - v.centre.y).abs() <= v.half.y.max(3.0) + 2.0
    };
    let off_view = |v: &LaidView| {
        ((p - v.centre).abs() - v.half - Point2::splat(2.0))
            .max(Point2::ZERO)
            .length()
    };
    match mode {
        PickMode::Views => {
            for v in laid.views.iter().filter(|v| !v.detail && in_view(v)) {
                // (The smallest view about it first: one inside another.)
                let size = v.half.x * v.half.y;
                found.push((0, size, SheetPick::View { view: v.id.clone() }));
            }
        }
        PickMode::Measure { only } => {
            let views = laid
                .views
                .iter()
                .filter(|v| !v.detail && only.is_none_or(|id| id == v.id));
            for v in views {
                for s in v.snaps.iter().filter(|s| s.distance(p) <= reach) {
                    let pick = SheetPick::Point {
                        view: v.id.clone(),
                        at: *s,
                    };
                    found.push((0, s.distance(p), pick));
                }
                for (c, r) in &v.rounds {
                    let off = (c.distance(p) - r).abs();
                    if off <= reach * 0.6 {
                        let pick = SheetPick::Round {
                            view: v.id.clone(),
                            centre: *c,
                            radius: *r,
                        };
                        found.push((1, off, pick));
                    }
                }
                for (a, b) in &v.lines {
                    let off = off_line(p, *a, *b);
                    if off <= reach * 0.6 {
                        let pick = SheetPick::Edge {
                            view: v.id.clone(),
                            ends: [*a, *b],
                        };
                        found.push((2, off, pick));
                    }
                }
            }
        }
        PickMode::Select { missing } => {
            const GRIP: u8 = 0;
            const THING: u8 = 1;
            const MISSING: u8 = 2;
            const MARK: u8 = 3;
            const VIEW: u8 = 4;
            const CELL: u8 = 5;
            for n in laid.notes.iter().filter(|n| placed(n)) {
                let (index, kind) = (n.index, n.kind.clone());
                let on_path = n
                    .path
                    .iter()
                    .map(|(a, b)| off_line(p, *a, *b))
                    .fold(f64::INFINITY, f64::min);
                match &n.kind {
                    Placing::Note | Placing::Own => {
                        let off = off_box(p, n.rect, 1.0);
                        if off == 0.0 {
                            found.push((GRIP, 0.0, SheetPick::Text { index, kind }));
                        } else if let Some(l) = n.linear {
                            // (By its lines: the dimension line, the
                            // extension lines.)
                            let lines = [
                                (l.line[0], l.line[1]),
                                (l.from[0], l.line[0]),
                                (l.from[1], l.line[1]),
                            ];
                            let off = lines
                                .iter()
                                .map(|(a, b)| off_line(p, *a, *b))
                                .fold(f64::INFINITY, f64::min);
                            if off <= reach * 0.5 {
                                found.push((THING, off, SheetPick::Note { index, kind }));
                            }
                        }
                        for k in 0..2 {
                            if let Some(at) = start_of(sheet, n, k)
                                && at.distance(p) <= reach
                            {
                                found.push((GRIP, at.distance(p), SheetPick::Start { index, k }));
                            }
                        }
                    }
                    Placing::Words => {
                        for (k, tip) in n.tips.iter().enumerate() {
                            if tip.distance(p) <= reach {
                                found.push((GRIP, tip.distance(p), SheetPick::Tip { index, k }));
                            }
                        }
                        let off = off_box(p, n.rect, 1.0);
                        if off == 0.0 {
                            found.push((THING, 0.0, SheetPick::Note { index, kind }));
                        }
                    }
                    Placing::Balloon(_) => {
                        if off_box(p, n.rect, 1.0) == 0.0 {
                            found.push((GRIP, 0.0, SheetPick::Text { index, kind }));
                        }
                    }
                    Placing::Line(_) => {
                        if let [(a, b)] = n.path[..] {
                            for (k, end) in [a, b].into_iter().enumerate() {
                                if end.distance(p) <= reach {
                                    found.push((
                                        GRIP,
                                        end.distance(p),
                                        SheetPick::End { index, k },
                                    ));
                                }
                            }
                        }
                        if on_path <= reach * 0.5 {
                            found.push((THING, on_path, SheetPick::Note { index, kind }));
                        }
                    }
                }
            }
            if missing {
                for (index, n) in laid.notes.iter().enumerate().filter(|(_, n)| !placed(n)) {
                    let on_path = n
                        .path
                        .iter()
                        .map(|(a, b)| off_line(p, *a, *b))
                        .fold(f64::INFINITY, f64::min);
                    let off = off_box(p, n.rect, 1.0).min(if on_path <= reach * 0.5 {
                        on_path
                    } else {
                        f64::INFINITY
                    });
                    if off.is_finite() && (off == 0.0 || on_path <= reach * 0.5) {
                        found.push((MISSING, off, SheetPick::Missing { index }));
                    }
                }
            }
            for view in &sheet.views {
                match &view.kind {
                    ViewKind::Section { .. } => {
                        let Some(cut) = laid.cuts.iter().find(|c| c.view == view.id) else {
                            continue;
                        };
                        let id = || view.id.clone();
                        if movable.contains(&view.id) {
                            for (index, at) in cut.path.iter().enumerate() {
                                if at.distance(p) <= reach {
                                    let pick = SheetPick::Corner { view: id(), index };
                                    found.push((GRIP, at.distance(p), pick));
                                }
                            }
                            for at in &cut.arrows {
                                // (An arrow on its corner is the corner.)
                                let index = arrow_corner(&cut.path, *at);
                                if at.distance(p) <= reach && cut.path[index].distance(*at) > 1e-6 {
                                    let pick = SheetPick::Arrow { view: id(), index };
                                    found.push((GRIP, at.distance(p), pick));
                                }
                            }
                        }
                        let off = cut
                            .drawn
                            .windows(2)
                            .map(|w| off_line(p, w[0], w[1]))
                            .chain(cut.arrows.iter().map(|a| a.distance(p)))
                            .fold(f64::INFINITY, f64::min);
                        if off <= reach * 0.5 {
                            found.push((MARK, off, SheetPick::CutLine { view: id() }));
                        }
                    }
                    ViewKind::Detail { .. } => {
                        let Some((middle, rim, r)) = detail_handles(laid, sheet, &view.id) else {
                            continue;
                        };
                        let id = || view.id.clone();
                        if middle.distance(p) <= reach {
                            found.push((
                                GRIP,
                                middle.distance(p),
                                SheetPick::Middle { view: id() },
                            ));
                        }
                        if rim.distance(p) <= reach {
                            found.push((GRIP, rim.distance(p), SheetPick::Rim { view: id() }));
                        }
                        let off = (middle.distance(p) - r).abs();
                        if off <= reach * 0.5 {
                            found.push((MARK, off, SheetPick::Circle { view: id() }));
                        }
                    }
                    _ => {}
                }
            }
            for v in &laid.views {
                if let Some(rect) = v.caption
                    && off_box(p, rect, 1.0) == 0.0
                {
                    found.push((GRIP, 0.0, SheetPick::Caption { view: v.id.clone() }));
                }
                if in_view(v) {
                    // (A detail before the view it lies over; the
                    // smaller of two before the larger.)
                    let off = off_view(v) + v.half.x * v.half.y * 1e-6;
                    found.push((VIEW, off, SheetPick::View { view: v.id.clone() }));
                }
            }
            for (index, c) in laid.cells.iter().enumerate() {
                if off_box(p, c.rect, 0.0) == 0.0 {
                    found.push((CELL, 0.0, SheetPick::Cell { index }));
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut picks: Vec<SheetPick> = Vec::new();
    for (_, _, pick) in found {
        if !picks.contains(&pick) {
            picks.push(pick);
        }
    }
    picks
}

/// What `pick` is called next to the pointer.
pub fn describe(of: &Sheeted<'_>, pick: &SheetPick) -> String {
    let Sheeted { sheet, laid, .. } = *of;
    let label = |view: &str| {
        laid.views
            .iter()
            .find(|v| v.id == view)
            .map_or_else(|| view.to_owned(), |v| v.label.clone())
    };
    let note = |index: usize, kind: &Placing| match kind {
        Placing::Note => sheet
            .notes
            .get(index)
            .map_or("Dimension".to_owned(), |n| format!("Dimension {}", n.note)),
        Placing::Own => format!("Sheet dimension {}", index + 1),
        Placing::Words => {
            let text = sheet.words.get(index).map_or("", |w| w.text.as_str());
            let short: String = text.chars().take(24).collect();
            format!("Words \"{short}\"")
        }
        Placing::Line(of) => of.label(),
        Placing::Balloon(part) => format!("Balloon {part}"),
    };
    match pick {
        SheetPick::Text { index, kind } => match kind {
            Placing::Balloon(_) => note(*index, kind),
            _ => format!("{}: text", note(*index, kind)),
        },
        SheetPick::Start { index, k } => {
            format!("{}: extension line {}", note(*index, &Placing::Note), k + 1)
        }
        SheetPick::Corner { view, index } => {
            format!("Cutting line of {}: point {}", label(view), index + 1)
        }
        SheetPick::Arrow { view, .. } => format!("Cutting line of {}: arrow", label(view)),
        SheetPick::Tip { index, k } => {
            format!("{}: leader {}", note(*index, &Placing::Words), k + 1)
        }
        SheetPick::End { index, k } => {
            let of = sheet
                .lines
                .get(*index)
                .map_or(String::new(), |l| l.of.label());
            format!("{of}: end {}", k + 1)
        }
        SheetPick::Middle { view } => format!("{}: middle", label(view)),
        SheetPick::Rim { view } => format!("{}: size of its circle", label(view)),
        SheetPick::Caption { view } => format!("{}: caption", label(view)),
        SheetPick::Note { index, kind } => note(*index, kind),
        SheetPick::Missing { index } => {
            let what = laid
                .notes
                .get(*index)
                .map_or(String::new(), |n| match &n.kind {
                    Placing::Line(of) => of.label(),
                    _ => n.preview.as_ref().map_or(String::new(), |p| p.0.clone()),
                });
            format!("Missing {what}: click to place it")
        }
        SheetPick::CutLine { view } => format!("Cutting line of {}", label(view)),
        SheetPick::Circle { view } => format!("Circle of {}", label(view)),
        SheetPick::View { view } => label(view),
        SheetPick::Cell { index } => laid
            .cells
            .get(*index)
            .map_or("Title block".to_owned(), |c| {
                format!("Title block: {}", c.label)
            }),
        SheetPick::Point { view, .. } => format!("Point of {}", label(view)),
        SheetPick::Round { view, .. } => format!("Circle of {}", label(view)),
        SheetPick::Edge { view, .. } => format!("Line of {}", label(view)),
    }
}

/// How the pick shows while it is the current candidate, on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    /// A box about it.
    Box([Point2; 2]),
    /// Its lines, over-drawn.
    Lines(Vec<(Point2, Point2)>),
    /// A handle: a small square.
    Handle(Point2),
    /// A circle about `.0` of radius `.1`.
    Ring(Point2, f64),
}

/// How `pick` is marked as the current candidate.
pub fn marks(of: &Sheeted<'_>, pick: &SheetPick) -> Vec<Mark> {
    let Sheeted { sheet, laid, .. } = *of;
    let laid_note = |index: usize, kind: &Placing| {
        laid.notes
            .iter()
            .find(|n| placed(n) && n.index == index && n.kind == *kind)
    };
    let cut = |view: &str| laid.cuts.iter().find(|c| c.view == view);
    match pick {
        SheetPick::Text { index, kind } => laid_note(*index, kind)
            .map(|n| vec![Mark::Box(n.rect)])
            .unwrap_or_default(),
        SheetPick::Note { index, kind } => {
            let Some(n) = laid_note(*index, kind) else {
                return Vec::new();
            };
            let mut lines = n.path.clone();
            if let Some(l) = n.linear {
                lines.extend([
                    (l.line[0], l.line[1]),
                    (l.from[0], l.line[0]),
                    (l.from[1], l.line[1]),
                ]);
            }
            let mut marks = vec![Mark::Lines(lines)];
            if n.path.is_empty() {
                marks.push(Mark::Box(n.rect));
            }
            marks
        }
        SheetPick::Missing { index } => laid
            .notes
            .get(*index)
            .map(|n| vec![Mark::Box(n.rect), Mark::Lines(n.path.clone())])
            .unwrap_or_default(),
        SheetPick::Start { index, k } => laid_note(*index, &Placing::Note)
            .and_then(|n| start_of(sheet, n, *k))
            .map(|at| vec![Mark::Handle(at)])
            .unwrap_or_default(),
        SheetPick::Corner { view, index } => cut(view)
            .and_then(|c| c.path.get(*index))
            .map(|at| vec![Mark::Handle(*at)])
            .unwrap_or_default(),
        SheetPick::Arrow { view, index } => cut(view)
            .and_then(|c| {
                let corner = c.path.get(*index)?;
                c.arrows
                    .iter()
                    .min_by(|a, b| a.distance(*corner).total_cmp(&b.distance(*corner)))
            })
            .map(|at| vec![Mark::Handle(*at)])
            .unwrap_or_default(),
        SheetPick::Tip { index, k } => laid_note(*index, &Placing::Words)
            .and_then(|n| n.tips.get(*k))
            .map(|at| vec![Mark::Handle(*at)])
            .unwrap_or_default(),
        SheetPick::End { index, k } => laid
            .notes
            .iter()
            .find(|n| placed(n) && n.index == *index && matches!(n.kind, Placing::Line(_)))
            .and_then(|n| n.path.first())
            .map(|(a, b)| vec![Mark::Handle(if *k == 0 { *a } else { *b })])
            .unwrap_or_default(),
        SheetPick::Middle { view } => detail_handles(laid, sheet, view)
            .map(|(m, _, _)| vec![Mark::Handle(m)])
            .unwrap_or_default(),
        SheetPick::Rim { view } => detail_handles(laid, sheet, view)
            .map(|(_, rim, _)| vec![Mark::Handle(rim)])
            .unwrap_or_default(),
        SheetPick::Caption { view } => laid
            .views
            .iter()
            .find(|v| v.id == *view)
            .and_then(|v| v.caption)
            .map(|r| vec![Mark::Box(r)])
            .unwrap_or_default(),
        SheetPick::CutLine { view } => cut(view)
            .map(|c| {
                vec![Mark::Lines(
                    c.drawn.windows(2).map(|w| (w[0], w[1])).collect(),
                )]
            })
            .unwrap_or_default(),
        SheetPick::Circle { view } => detail_handles(laid, sheet, view)
            .map(|(m, _, r)| vec![Mark::Ring(m, r)])
            .unwrap_or_default(),
        SheetPick::View { view } => laid
            .views
            .iter()
            .find(|v| v.id == *view)
            .map(|v| vec![Mark::Box([v.centre - v.half, v.centre + v.half])])
            .unwrap_or_default(),
        SheetPick::Cell { index } => laid
            .cells
            .get(*index)
            .map(|c| vec![Mark::Box(c.rect)])
            .unwrap_or_default(),
        SheetPick::Point { at, .. } => vec![Mark::Ring(*at, 0.0)],
        SheetPick::Round { centre, radius, .. } => vec![Mark::Ring(*centre, *radius)],
        SheetPick::Edge { ends, .. } => vec![Mark::Lines(vec![(ends[0], ends[1])])],
    }
}

/// The candidates under a resting pointer and the one gone on to: as
/// long as the pointer stays where it was (within a slack) and the
/// candidates are the same, the one cycled to stays.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hovering {
    /// Where the pointer was when they were found.
    at: Option<Point2>,
    /// The candidates, the first to pick first.
    pub list: Vec<SheetPick>,
    /// The current one.
    pub current: usize,
}

impl Hovering {
    /// The candidates `list` under the pointer at `at`: the current one
    /// stays where the pointer is within `slack` of where it was and the
    /// candidates are the same; else it is the first.
    pub fn update(&mut self, at: Option<Point2>, list: Vec<SheetPick>, slack: f64) {
        let rests = matches!((at, self.at), (Some(a), Some(b)) if a.distance(b) <= slack);
        if !rests || list != self.list {
            self.current = 0;
            self.at = at;
        }
        self.list = list;
    }

    /// Goes on to the next candidate (after the last, the first).
    pub fn cycle(&mut self) {
        if !self.list.is_empty() {
            self.current = (self.current + 1) % self.list.len();
        }
    }

    /// The current candidate.
    pub fn current(&self) -> Option<&SheetPick> {
        self.list.get(self.current)
    }

    /// Forgets them: the pointer left.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shapecraft_ops_drawing::lay::{LaidCut, LaidLinear};
    use shapecraft_ops_drawing::sheet::{PlacedNote, SheetView, Side, Style};

    fn p(x: f64, y: f64) -> Point2 {
        Point2::new(x, y)
    }

    fn view(id: &str, kind: ViewKind) -> SheetView {
        SheetView {
            id: id.into(),
            kind,
            at: [100.0, 100.0],
            scale: None,
            style: Style::HiddenRemoved,
            bodies: Vec::new(),
            cut_only: false,
            half: None,
            section: None,
            caption_at: None,
        }
    }

    fn laid_view(id: &str, centre: Point2, half: Point2) -> LaidView {
        LaidView {
            id: id.into(),
            centre,
            half,
            scale: 1.0,
            anchor: centre,
            origin: Point2::ZERO,
            detail: false,
            snaps: Vec::new(),
            lines: Vec::new(),
            rounds: Vec::new(),
            label: id.into(),
            caption: None,
            cut_shows_in: Vec::new(),
        }
    }

    /// A front view 40 × 40 about (100, 100) with a dimension 25 above
    /// it, a section of it whose cutting line runs up through its
    /// middle — its corners on the view, its arrows past it — with its
    /// caption, and two dimensions missing near (100, 60).
    fn sheet() -> (Sheet, Laid) {
        let sheet = Sheet {
            views: vec![
                view("front", ViewKind::Base { side: Side::Front }),
                view(
                    "cut",
                    ViewKind::Section {
                        of: Some("front".into()),
                        section: "s".into(),
                        letter: "B".into(),
                        turn: 0.0,
                    },
                ),
            ],
            notes: vec![PlacedNote {
                note: "sketch1/d1".into(),
                view: "front".into(),
                at: [0.0, 30.0],
                decimals: None,
                starts: [None; 2],
                count: None,
            }],
            ..Sheet::default()
        };
        let mut cut = laid_view("cut", p(200.0, 100.0), p(20.0, 20.0));
        cut.caption = Some([p(195.0, 125.0), p(205.0, 130.0)]);
        let note = |preview: Option<&str>, rect: [Point2; 2]| LaidNote {
            index: 0,
            kind: Placing::Note,
            rect,
            view: "front".into(),
            preview: preview.map(|n| (n.to_owned(), [0.0, 0.0])),
            linear: None,
            path: Vec::new(),
            tips: Vec::new(),
        };
        let mut placed = note(None, [p(95.0, 128.0), p(105.0, 132.0)]);
        placed.linear = Some(LaidLinear {
            from: [p(80.0, 120.0), p(120.0, 120.0)],
            ways: [Point2::Y, Point2::Y],
            line: [p(80.0, 130.0), p(120.0, 130.0)],
            room: 6.0,
        });
        let laid = Laid {
            views: vec![laid_view("front", p(100.0, 100.0), p(20.0, 20.0)), cut],
            notes: vec![
                placed,
                note(Some("sketch1/d2"), [p(95.0, 58.0), p(105.0, 62.0)]),
                note(Some("sketch1/d3"), [p(96.0, 59.0), p(106.0, 63.0)]),
            ],
            cuts: vec![LaidCut {
                view: "cut".into(),
                of: "front".into(),
                path: vec![p(100.0, 90.0), p(100.0, 110.0)],
                drawn: vec![p(100.0, 72.0), p(100.0, 128.0)],
                arrows: vec![p(100.0, 72.0), p(100.0, 128.0)],
            }],
            ..Laid::default()
        };
        (sheet, laid)
    }

    #[test]
    fn candidates_come_handles_first_then_things_missing_lines_and_views() {
        let (sheet, laid) = sheet();
        let movable = vec!["cut".to_owned()];
        let of = Sheeted {
            sheet: &sheet,
            laid: &laid,
            movable: &movable,
        };
        let select = PickMode::Select { missing: false };
        // On a corner of the cutting line, over the view: the corner,
        // then the line, then the view.
        assert_eq!(
            candidates(&of, select, p(100.2, 90.0), 2.0),
            vec![
                SheetPick::Corner {
                    view: "cut".into(),
                    index: 0
                },
                SheetPick::CutLine { view: "cut".into() },
                SheetPick::View {
                    view: "front".into()
                },
            ]
        );
        // On an arrow, off the view: the arrow (it moves the corner
        // nearer it), then the line.
        assert_eq!(
            candidates(&of, select, p(100.0, 72.5), 2.0),
            vec![
                SheetPick::Arrow {
                    view: "cut".into(),
                    index: 0
                },
                SheetPick::CutLine { view: "cut".into() },
            ]
        );
        // A dimension's text; its line; its extension line's start.
        assert_eq!(
            candidates(&of, select, p(103.0, 130.0), 2.0),
            vec![SheetPick::Text {
                index: 0,
                kind: Placing::Note
            }]
        );
        assert_eq!(
            candidates(&of, select, p(90.0, 130.3), 2.0),
            vec![SheetPick::Note {
                index: 0,
                kind: Placing::Note
            }]
        );
        assert_eq!(
            candidates(&of, select, p(80.0, 121.0), 2.0),
            vec![
                SheetPick::Start { index: 0, k: 0 },
                SheetPick::Note {
                    index: 0,
                    kind: Placing::Note
                },
                SheetPick::View {
                    view: "front".into()
                },
            ]
        );
        // The caption of the section, over its view.
        assert_eq!(
            candidates(&of, select, p(200.0, 127.0), 2.0)[0],
            SheetPick::Caption { view: "cut".into() }
        );
        // What is missing only while it shows: before the cutting line
        // and the view, the nearer first.
        assert_eq!(
            candidates(&of, select, p(100.0, 60.0), 2.0),
            Vec::<SheetPick>::new()
        );
        let missing = PickMode::Select { missing: true };
        assert_eq!(
            candidates(&of, missing, p(100.0, 60.0), 2.0),
            vec![
                SheetPick::Missing { index: 1 },
                SheetPick::Missing { index: 2 },
            ]
        );
        assert_eq!(
            candidates(&of, missing, p(100.0, 80.0), 2.0),
            vec![
                SheetPick::CutLine { view: "cut".into() },
                SheetPick::View {
                    view: "front".into()
                },
            ]
        );
        // A line that cannot be dragged has neither corners nor arrows.
        let fixed = Sheeted { movable: &[], ..of };
        assert_eq!(
            candidates(&fixed, select, p(100.0, 90.0), 2.0),
            vec![
                SheetPick::CutLine { view: "cut".into() },
                SheetPick::View {
                    view: "front".into()
                },
            ]
        );
        // Picking views only (a tool that draws in one): the view.
        assert_eq!(
            candidates(&of, PickMode::Views, p(100.0, 90.0), 2.0),
            vec![SheetPick::View {
                view: "front".into()
            }]
        );
    }

    #[test]
    fn the_right_button_goes_on_to_the_next_while_the_pointer_rests() {
        let list = vec![
            SheetPick::Missing { index: 1 },
            SheetPick::Missing { index: 2 },
        ];
        let mut h = Hovering::default();
        h.update(Some(p(10.0, 10.0)), list.clone(), 0.5);
        assert_eq!(h.current(), Some(&list[0]));
        h.cycle();
        h.update(Some(p(10.1, 10.0)), list.clone(), 0.5);
        assert_eq!(h.current(), Some(&list[1]));
        h.cycle();
        assert_eq!(h.current(), Some(&list[0]));
        h.cycle();
        // Moved on: the first again.
        h.update(Some(p(12.0, 10.0)), list.clone(), 0.5);
        assert_eq!(h.current(), Some(&list[0]));
        // Other candidates: the first of them.
        h.cycle();
        h.update(Some(p(12.0, 10.0)), list[..1].to_vec(), 0.5);
        assert_eq!(h.current(), Some(&list[0]));
    }
}
