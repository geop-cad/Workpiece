//! A drawing sheet laid out: everything that is drawn of it, in
//! millimetres from the sheet's lower left corner — the frame and the
//! title block (ISO 5457, ISO 7200), the views of the model (ISO 128),
//! and what is said of the model in them: dimensions (ISO 129-1), datum
//! features (ISO 5459) and geometric tolerances (ISO 1101).

mod drawn;
mod export;
mod frame;
mod lines;
mod notes;
pub mod parts;
mod pdf;
pub(crate) mod threads;
mod views;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::Sheet;
use crate::sheet::ViewKind;
use workpiece_core::Unit;
use workpiece_core::math::Point2;
use workpiece_core::world::World;

pub use frame::{Weight, mass_text, solid_bodies, weight};
pub use notes::on_model;
pub use threads::Threads;
pub use views::Seen;

/// The colour of what is not on the sheet yet, shown as what it would
/// be.
pub const PREVIEW: [u8; 3] = [0, 110, 220];

thread_local! {
    /// The height of the lettering of the sheet being laid out.
    static LETTERING: std::cell::Cell<f64> = const { std::cell::Cell::new(3.5) };
}

/// The height of the lettering of dimensions and notes (ISO 3098), in
/// millimetres: the sheet's ([`Drafting::letter`](crate::sheet::Drafting)),
/// while it is laid out.
pub fn lettering() -> f64 {
    LETTERING.with(std::cell::Cell::get)
}

/// The height of the lettering of views' names, detail and section
/// letters and balloons' numbers: the next of ISO 3098's heights over
/// [`lettering`] (5 over 3.5).
pub fn title_letter() -> f64 {
    let own = lettering();
    [2.5, 3.5, 5.0, 7.0, 10.0, 14.0, 20.0]
        .into_iter()
        .find(|h| *h > own + 1e-9)
        .unwrap_or(own * 1.4)
}

/// How wide `text` is on the sheet, its capitals `height` high: as the
/// PDF and the SVG set it (Helvetica), and as what follows it on its
/// line is laid out by. A screen with letters of its own sets it no
/// wider.
pub fn text_width(text: &str, height: f64) -> f64 {
    notes::width(text, height)
}

/// `make`, with the lettering `height` (see [`lettering`]).
fn lettered<T>(height: f64, make: impl FnOnce() -> T) -> T {
    let before = LETTERING.with(|l| l.replace(height));
    let made = make();
    LETTERING.with(|l| l.set(before));
    made
}

/// What a line is drawn with (ISO 128-2): how wide, and whether
/// broken. How wide is of the sheet's line group: the wide lines as
/// wide as it says, the narrow half that (ISO 128-2: 2:1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pen {
    /// The frame of the sheet: continuous, 0.7 wide (ISO 5457).
    Frame,
    /// Edges in view: continuous wide (01.2).
    Wide,
    /// Dimension, extension and leader lines, hatching, the outlines
    /// of details: continuous narrow (01.1).
    Narrow,
    /// Hidden edges: dashed narrow (02.1).
    Dashed,
    /// Centre lines, lines of symmetry, cutting planes: long-dashed
    /// dotted narrow (04.1).
    Chain,
    /// The ends of a cutting plane: long-dashed dotted wide (04.2).
    ChainWide,
    /// Adjacent parts, other positions of moving parts: long-dashed
    /// double-dotted narrow (05.1).
    Phantom,
}

impl Pen {
    /// How wide it is, in millimetres, on a sheet whose wide lines are
    /// `wide` wide.
    pub fn width(self, wide: f64) -> f64 {
        let wide = if wide > 0.0 { wide } else { 0.5 };
        match self {
            Pen::Frame => 0.7,
            Pen::Wide | Pen::ChainWide => wide,
            Pen::Narrow | Pen::Dashed | Pen::Chain | Pen::Phantom => wide / 2.0,
        }
    }

    /// Its dashes and gaps in turn, in millimetres, on a sheet whose
    /// wide lines are `wide` wide; none for a continuous line. ISO
    /// 128-2, in the line's own width d: a dash 12d, a gap 3d, a long
    /// dash 24d, a dot 0.5d.
    pub fn dashes(self, wide: f64) -> Vec<f64> {
        let d = self.width(wide);
        match self {
            Pen::Frame | Pen::Wide | Pen::Narrow => Vec::new(),
            Pen::Dashed => vec![12.0 * d, 3.0 * d],
            Pen::Chain | Pen::ChainWide => vec![24.0 * d, 3.0 * d, 0.5 * d, 3.0 * d],
            Pen::Phantom => vec![24.0 * d, 3.0 * d, 0.5 * d, 3.0 * d, 0.5 * d, 3.0 * d],
        }
    }

    /// Its dashes and gaps (as [`Pen::dashes`]) for a line `length`
    /// long: of a chain line, its long dashes stretched or shrunk so that
    /// it starts and ends with a long dash, its short ones and gaps as
    /// they are (ISO 128-2: a centre line starts and ends with a long
    /// dash) — else an axis shorter than a long dash and a half is one
    /// long dash, as good as continuous. Others as they are.
    pub fn fitted_dashes(self, wide: f64, length: f64) -> Vec<f64> {
        let dashes = self.dashes(wide);
        if !matches!(self, Pen::Chain | Pen::ChainWide | Pen::Phantom) || dashes.is_empty() {
            return dashes;
        }
        let long = dashes[0];
        // (What comes between two long dashes: gaps and dots.)
        let between: f64 = dashes[1..].iter().sum();
        // As many long dashes as fit (two at least), each at least half
        // of what it is.
        // (An odd number, where they fit: crossing lines — a centre mark
        // — cross at a long dash, in the middle.)
        let n = ((length + between) / (long + between)).round().max(2.0);
        let odd = if n % 2.0 == 0.0 { n + 1.0 } else { n };
        let fits = |n: f64| (length - (n - 1.0) * between) / n >= long * 0.25;
        let n = if fits(odd) { odd } else { n };
        let each = (length - (n - 1.0) * between) / n;
        if each < long * 0.25 {
            return dashes;
        }
        let mut fitted = dashes;
        fitted[0] = each;
        fitted
    }

    /// Its dashes and gaps as they are set for a line with round ends
    /// (a PDF's, an SVG's, the screen's): each dash shorter, each gap
    /// longer by the line's width, so that what shows is as long as
    /// [`Pen::dashes`] says — a dot a round dot.
    pub fn round_dashes(self, wide: f64) -> Vec<f64> {
        self.rounded(wide, self.dashes(wide))
    }

    /// `dashes` (of this pen) as they are set for a line with round
    /// ends: see [`Pen::round_dashes`].
    pub fn rounded(self, wide: f64, dashes: Vec<f64>) -> Vec<f64> {
        let d = self.width(wide);
        dashes
            .iter()
            .enumerate()
            .map(|(k, x)| match k % 2 {
                0 => (x - d).max(0.0),
                _ => x + d,
            })
            .collect()
    }
}

/// A straight line of the drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// From here.
    pub a: Point2,
    /// To here.
    pub b: Point2,
    /// With this pen.
    pub pen: Pen,
    /// Whether it is of something that is not on the sheet yet, shown
    /// as what it would be.
    pub preview: bool,
}

/// A line of text of the drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// Where it is.
    pub at: Point2,
    /// What it says.
    pub text: String,
    /// The height of its capitals, in millimetres.
    pub height: f64,
    /// Which point of it is at `at`: 0, 0 its lower left, 0.5, 0.5 its
    /// middle, 1, 1 its upper right.
    pub anchor: [f64; 2],
    /// How it is turned about that point, counter-clockwise, in
    /// radians.
    pub angle: f64,
    /// Whether it is of something that is not on the sheet yet, shown
    /// as what it would be.
    pub preview: bool,
}

/// A filled area of the drawing: a triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill {
    /// Its corners.
    pub corners: [Point2; 3],
    /// Its colour: red, green and blue.
    pub colour: [u8; 3],
}

/// A view as it is laid out: what the user points at and drags.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidView {
    /// Its name on the sheet.
    pub id: String,
    /// Its middle on the sheet.
    pub centre: Point2,
    /// Half its width and its height on the sheet.
    pub half: Point2,
    /// How many millimetres of the sheet one length of the model is.
    pub scale: f64,
    /// Where the view itself is on the sheet: what stands in it is
    /// placed from here.
    pub anchor: Point2,
    /// The point of the view's own coordinates that is at `anchor`.
    pub origin: Point2,
    /// Whether it is a part of another view, larger.
    pub detail: bool,
    /// The ends of its lines, and the middles of what is round in it,
    /// on the sheet: what a dimension of the sheet's own is put
    /// between, and what is drawn by hand snaps to.
    pub snaps: Vec<Point2>,
    /// Its lines in view, on the sheet.
    pub lines: Vec<(Point2, Point2)>,
    /// What is round in it and seen square on: middle and radius, on
    /// the sheet.
    pub rounds: Vec<(Point2, f64)>,
    /// What it is, for the list of views: `Front`, `Section A–A`.
    pub label: String,
    /// The lower left and the upper right corner of its caption
    /// (`B–B`, `A (5:1)`, a scale of its own), if it has one: what is
    /// pointed at to drag it.
    pub caption: Option<[Point2; 2]>,
    /// Of a section: the other views that see its cut edge on — where
    /// its cutting line could show (see [`crate::sheet::ViewKind::Section`]'s
    /// `of`).
    pub cut_shows_in: Vec<String>,
}

/// Something placed on the sheet, as it is laid out: where its text
/// is, to be pointed at and dragged.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidNote {
    /// Which it is: its place among what the sheet has of its kind
    /// (its notes, its own dimensions, its words, its lines).
    pub index: usize,
    /// What it is.
    pub kind: Placing,
    /// The lower left and the upper right corner of its text.
    pub rect: [Point2; 2],
    /// The view it is in.
    pub view: String,
    /// Of something said of the model that is not on the sheet, shown
    /// as what it would be: its name in the model, and where its text
    /// would stand, in millimetres from the view's middle.
    pub preview: Option<(String, [f64; 2])>,
    /// Of a length: how it is drawn, to drag its parts by.
    pub linear: Option<LaidLinear>,
    /// Of a line: its pieces, on the sheet — what is pointed at, and
    /// whose first and last ends are dragged.
    pub path: Vec<(Point2, Point2)>,
    /// Of words with leaders (or a balloon): where each leader ends, to
    /// be dragged.
    pub tips: Vec<Point2>,
}

/// What a [`LaidNote`] is.
#[derive(Debug, Clone, PartialEq)]
pub enum Placing {
    /// Something said of the model: one of the sheet's notes.
    Note,
    /// One of the sheet's own dimensions.
    Own,
    /// Words written on the sheet.
    Words,
    /// A centre line or the trace of a datum: one of the sheet's lines,
    /// or — shown as what it would be — one it could have.
    Line(crate::sheet::LineOf),
    /// The balloon of the item a part is (by the name it is placed
    /// under): `index` is its place among the sheet's balloons, or
    /// `usize::MAX` for one put by itself.
    Balloon(String),
}

/// A length as it is drawn: what its extension lines run from and
/// which way, and where its dimension line runs between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaidLinear {
    /// The two points it measures between, on the sheet.
    pub from: [Point2; 2],
    /// The way each extension line runs from there, to the dimension
    /// line (unit vectors; nothing where the line runs through it).
    pub ways: [Point2; 2],
    /// The dimension line's ends, where the extension lines meet it.
    pub line: [Point2; 2],
    /// How much room its text takes along the line.
    pub room: f64,
}

impl LaidLinear {
    /// Where the extension line `k` (0 or 1) starts, as far from what
    /// it measures, for the pointer at `p`: snapped, within `reach`, to
    /// the sheet's `gap` off the outline and to where the line crosses
    /// one of `lines` (those of its view).
    pub fn start_at(
        &self,
        k: usize,
        p: Point2,
        gap: f64,
        lines: &[(Point2, Point2)],
        reach: f64,
    ) -> f64 {
        let (from, way) = (self.from[k], self.ways[k]);
        let end = from.distance(self.line[k]);
        let s = (p - from).dot(way).clamp(0.0, end);
        let mut near = vec![gap];
        for (a, b) in lines {
            // (Where the extension line, as a ray, crosses the line.)
            let d = *b - *a;
            let det = way.perp_dot(d);
            if det.abs() < 1e-12 {
                continue;
            }
            let t = (*a - from).perp_dot(d) / det;
            let u = (*a - from).perp_dot(way) / det;
            if (0.0..=1.0).contains(&u) && t > 1e-9 && t <= end {
                near.push(t);
            }
        }
        near.into_iter()
            .filter(|t| (t - s).abs() <= reach)
            .min_by(|x, y| (x - s).abs().total_cmp(&(y - s).abs()))
            .unwrap_or(s)
    }

    /// Where its text stands for the pointer at `p`: along its line
    /// snapped to the line's middle and to its ends (the text just
    /// inside each); and the line's height off what it measures
    /// snapped to `pitch` and its multiples off the outline, and to a
    /// `pitch` above or below — or level with — each of `others` (the
    /// other lengths of its view) that runs the same way. Within
    /// `reach` of where it would be unsnapped.
    pub fn text_at(&self, p: Point2, pitch: f64, others: &[LaidLinear], reach: f64) -> Point2 {
        let [a, b] = self.line;
        let along = match (b - a).try_normalize() {
            Some(along) => along,
            None => return p,
        };
        let up = along.perp();
        let length = a.distance(b);
        // Along the line.
        let t = (p - a).dot(along);
        let half = (self.room / 2.0 + 1.0).min(length / 2.0);
        let t = [length / 2.0, half, length - half]
            .into_iter()
            .filter(|c| (c - t).abs() <= reach)
            .min_by(|x, y| (x - t).abs().total_cmp(&(y - t).abs()))
            .unwrap_or(t);
        // Its height, across it.
        let h = p.dot(up);
        let side = if (a - self.from[0]).dot(up) < 0.0 {
            -1.0
        } else {
            1.0
        };
        let outline = self
            .from
            .iter()
            .map(|f| f.dot(up) * side)
            .fold(f64::NEG_INFINITY, f64::max)
            * side;
        let mut near: Vec<f64> = (1..=8)
            .map(|k| outline + side * pitch * f64::from(k))
            .collect();
        for other in others {
            let [oa, ob] = other.line;
            let Some(way) = (ob - oa).try_normalize() else {
                continue;
            };
            if way.perp_dot(along).abs() > 1e-6 {
                continue;
            }
            let level = oa.dot(up);
            near.extend([level - pitch, level, level + pitch]);
        }
        let h = near
            .into_iter()
            .filter(|c| (c - h).abs() <= reach)
            .min_by(|x, y| (x - h).abs().total_cmp(&(y - h).abs()))
            .unwrap_or(h);
        a + along * t + up * (h - a.dot(up))
    }
}

/// The line a section cuts along, as it shows in the view it is of.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidCut {
    /// The section view.
    pub view: String,
    /// The view the line shows in.
    pub of: String,
    /// The corners of the line as the part's section has them, on the
    /// sheet.
    pub path: Vec<Point2>,
    /// The line as it is drawn, on the sheet: one straight cut goes
    /// all across the view, a little past it (and on to its corners).
    pub drawn: Vec<Point2>,
    /// Where its arrows point at it: the ends of the drawn line that
    /// have one.
    pub arrows: Vec<Point2>,
}

/// A cell of the title block as it is laid out: what the user points
/// at to say what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidCell {
    /// The lower left and the upper right corner of it.
    pub rect: [Point2; 2],
    /// What it is called.
    pub label: String,
    /// What it says.
    pub value: String,
    /// The property it says, if it says one; else text of its own.
    pub property: Option<String>,
}

/// A sheet laid out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Laid {
    /// The sheet's width and height.
    pub size: [f64; 2],
    /// How wide its wide lines are, in millimetres (see [`Pen::width`]).
    pub wide: f64,
    /// Filled areas, drawn first.
    pub fills: Vec<Fill>,
    /// Lines.
    pub strokes: Vec<Stroke>,
    /// Text.
    pub texts: Vec<Text>,
    /// The views.
    pub views: Vec<LaidView>,
    /// What is placed on it.
    pub notes: Vec<LaidNote>,
    /// Where the sections cut, as their lines show in other views.
    pub cuts: Vec<LaidCut>,
    /// The cells of the title block, in the order the sheet has them.
    pub cells: Vec<LaidCell>,
    /// What is said of the model that stands nowhere on it yet, by
    /// name.
    pub missing: Vec<String>,
    /// What could not be drawn, and why.
    pub problems: Vec<String>,
}

impl Laid {
    fn line(&mut self, a: Point2, b: Point2, pen: Pen) {
        self.strokes.push(Stroke {
            a,
            b,
            pen,
            preview: false,
        });
    }

    fn text(&mut self, at: Point2, text: impl Into<String>, height: f64, anchor: [f64; 2]) {
        self.texts.push(Text {
            at,
            text: text.into(),
            height,
            anchor,
            angle: 0.0,
            preview: false,
        });
    }

    /// A circle about `centre`, as short lines.
    fn circle(&mut self, centre: Point2, radius: f64, pen: Pen) {
        self.arc(centre, radius, 0.0, std::f64::consts::TAU, pen);
    }

    /// An arc about `centre` from the angle `from` on by `sweep`, as
    /// short lines.
    fn arc(&mut self, centre: Point2, radius: f64, from: f64, sweep: f64, pen: Pen) {
        let steps = ((sweep.abs() * radius / 1.5).ceil() as usize).clamp(8, 180);
        let at =
            |k: usize| centre + Point2::from_angle(from + sweep * k as f64 / steps as f64) * radius;
        for k in 0..steps {
            self.line(at(k), at(k + 1), pen);
        }
    }

    /// A filled arrowhead (ISO 129-1:2018, "closed 30° filled": its
    /// sides 30° apart) with its tip at `tip`, pointing along `way`.
    fn arrow(&mut self, tip: Point2, way: Point2, length: f64) {
        let way = way.normalize_or_zero();
        let across = way.perp() * (length * (15.0_f64).to_radians().tan());
        let back = tip - way * length;
        self.fills.push(Fill {
            corners: [tip, back + across, back - across],
            colour: [0, 0, 0],
        });
    }
}

/// The lines of `strokes` joined where one ends where another starts
/// (or ends), as runs of points: what is drawn as one path, so that a
/// dashed line keeps its dashes going around a curve drawn of many
/// short pieces rather than starting afresh, a dash, at each.
pub(crate) fn chains<'a>(strokes: impl Iterator<Item = &'a Stroke>) -> Vec<Vec<Point2>> {
    let key = |p: Point2| ((p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64);
    // (A line drawn twice, either way, once: else a dashed one run there
    // and back has its dashes over its gaps, and is drawn solid.)
    let mut once = std::collections::HashSet::new();
    let lines: Vec<(Point2, Point2)> = strokes
        .map(|s| (s.a, s.b))
        .filter(|(a, b)| {
            let (ka, kb) = (key(*a), key(*b));
            once.insert(if ka <= kb { (ka, kb) } else { (kb, ka) })
        })
        .collect();
    let mut ends: std::collections::HashMap<(i64, i64), Vec<usize>> = Default::default();
    for (k, (a, b)) in lines.iter().enumerate() {
        ends.entry(key(*a)).or_default().push(k);
        ends.entry(key(*b)).or_default().push(k);
    }
    let mut used = vec![false; lines.len()];
    let mut out = Vec::new();
    for first in 0..lines.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut run = std::collections::VecDeque::from([lines[first].0, lines[first].1]);
        // (On from its end, then back from its start.)
        for forward in [true, false] {
            loop {
                let tip = if forward { run[run.len() - 1] } else { run[0] };
                // (Of those that go on from there, the one that goes on
                // straightest: where lines cross or branch, a line keeps
                // its dashes running on along itself, not round a
                // corner into another — which would leave a short piece
                // of it a chain by itself, a dash drawn solid.)
                let before = if forward {
                    tip - run[run.len() - 2]
                } else {
                    tip - run[1]
                }
                .normalize_or_zero();
                let next = ends.get(&key(tip)).and_then(|ks| {
                    ks.iter().copied().filter(|k| !used[*k]).max_by(|x, y| {
                        let on = |k: usize| {
                            let (a, b) = lines[k];
                            let far = if key(a) == key(tip) { b } else { a };
                            (far - tip).normalize_or_zero().dot(before)
                        };
                        on(*x).total_cmp(&on(*y))
                    })
                });
                let Some(k) = next else {
                    break;
                };
                used[k] = true;
                let (a, b) = lines[k];
                let far = if key(a) == key(tip) { b } else { a };
                if forward {
                    run.push_back(far);
                } else {
                    run.push_front(far);
                }
            }
        }
        out.push(run.into_iter().collect());
    }
    out
}

/// What a sheet is drawn from: the model, and what the part says of
/// itself.
pub struct Subject<'a> {
    /// The model.
    pub world: &'a World,
    /// The unit its lengths are in.
    pub unit: Unit,
    /// What it is made of, if all of it is made of one material.
    pub material: Option<String>,
    /// The part's name.
    pub name: &'a str,
    /// What the part says of itself, by name: its own properties; the
    /// built-in ones (see [`BUILT_IN`](crate::template::BUILT_IN)) it
    /// does not give are worked out of the rest — `Title` is its name,
    /// `Mass` what its bodies weigh.
    pub properties: &'a BTreeMap<String, String>,
    /// What the parts placed in it are called — the name of the part's
    /// file, or the title it gives itself — by the names they are
    /// placed under: what its parts list says (else the name they are
    /// placed under).
    pub parts: &'a BTreeMap<String, String>,
    /// How dense the materials are, by name, in kilograms a cubic metre:
    /// what a parts list with a density column says.
    pub densities: &'a BTreeMap<String, f64>,
}

/// What views show of worlds, kept while the world and the view are the
/// same: working out what is hidden takes long.
#[derive(Debug, Clone, Default)]
pub struct Kept(Arc<Mutex<BTreeMap<String, Arc<Seen>>>>);

impl Kept {
    /// The number kept as `key`, or what `make` makes, kept from now
    /// on: for what is worked out of a world and takes long.
    pub fn number(&self, key: String, make: impl FnOnce() -> f64) -> f64 {
        let seen = self.seen(key, || Seen {
            lo: Point2::new(make(), 0.0),
            ..Seen::default()
        });
        seen.lo.x
    }

    /// How many things are kept: for tests.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether something is kept as `key`.
    fn has(&self, key: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(key)
    }

    /// Keeps `seen` as `key`.
    fn put(&self, key: String, seen: Seen) {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if kept.len() > 64 {
            kept.clear();
        }
        kept.insert(key, Arc::new(seen));
    }

    fn seen(&self, key: String, make: impl FnOnce() -> Seen) -> Arc<Seen> {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(seen) = kept.get(&key) {
            return seen.clone();
        }
        // (No more than a sheet or two has.)
        if kept.len() > 64 {
            kept.clear();
        }
        let seen = Arc::new(make());
        kept.insert(key, seen.clone());
        seen
    }
}

/// How many millimetres of the sheet one length of the model is, at the
/// scale `scale`.
fn millimetres(scale: [u32; 2], unit: Unit) -> f64 {
    f64::from(scale[0].max(1)) / f64::from(scale[1].max(1)) * unit.millimetres()
}

/// A scale as it is written: `1:2`.
pub fn scale_text(scale: [u32; 2]) -> String {
    format!("{}:{}", scale[0].max(1), scale[1].max(1))
}

/// Lays `sheet` out as a drawing of `subject`; `kept` keeps what its
/// views show from one time to the next. With `previews`, what is said
/// of the model and is not on the sheet is drawn as well, as what it
/// would be ([`Stroke::preview`]): in the view named, or in every view
/// that sees it true.
pub fn lay_out(
    sheet: &Sheet,
    subject: &Subject<'_>,
    kept: &Kept,
    previews: Option<Option<&str>>,
) -> Laid {
    // Its numbers are written the sheet's way.
    let height = if sheet.drafting.letter > 0.0 {
        sheet.drafting.letter
    } else {
        3.5
    };
    workpiece_core::world::annotation::written(sheet.numbers, || {
        lettered(height, || laid_out(sheet, subject, kept, previews))
    })
}

fn laid_out(
    sheet: &Sheet,
    subject: &Subject<'_>,
    kept: &Kept,
    previews: Option<Option<&str>>,
) -> Laid {
    let mut laid = Laid {
        size: sheet.size(),
        wide: sheet.drafting.wide(sheet.format),
        ..Laid::default()
    };
    // (ISO 5455: 1, 2 and 5 and those times ten, a hundred: 1:2, 5:1,
    // 1:20. Another is drawn as asked, and said.)
    let iso = |n: u32| {
        let mut n = n.max(1);
        while n.is_multiple_of(10) {
            n /= 10;
        }
        matches!(n, 1 | 2 | 5)
    };
    let scales = std::iter::once(sheet.scale).chain(sheet.views.iter().filter_map(|v| v.scale));
    for scale in scales {
        if !(iso(scale[0]) && iso(scale[1]) && (scale[0] == 1 || scale[1] == 1)) {
            laid.problems.push(format!(
                "the scale {} is none of ISO 5455's (1:1, 1:2, 1:5, 1:10, 2:1, 5:1, 10:1 and those times ten): \
                 use one of them",
                scale_text(scale)
            ));
        }
    }
    frame::frame(&mut laid, sheet);
    frame::title_block(&mut laid, sheet, subject);
    // (What is written from here on: of the views and what stands in
    // them, not of the frame and the title block.)
    let own_texts = laid.texts.len();
    // The views: each from what it sees, where it stands.
    let frames = views::frames(sheet, subject, kept, &mut laid.problems);
    for placed in &frames {
        views::draw(&mut laid, sheet, placed, &frames);
    }
    drawn::draw(&mut laid, sheet);
    lines::draw(&mut laid, sheet, subject.world, &frames, previews);
    notes::draw(&mut laid, sheet, subject, &frames, previews);
    lines::labels(&mut laid);
    views::detail_letters(&mut laid, &frames);
    parts::draw(&mut laid, sheet, subject, &frames);
    overlaps(&mut laid, own_texts);
    laid
}

/// Tells in `laid`'s problems what is written over something else: a
/// text over another (from the `from`-th on: not the frame's and the
/// title block's own), a text or a view over the title block.
fn overlaps(laid: &mut Laid, from: usize) {
    let shrink = |[lo, hi]: [Point2; 2], by: f64| [lo + Point2::splat(by), hi - Point2::splat(by)];
    let meet = |a: &[Point2; 2], b: &[Point2; 2]| {
        a[0].x < b[1].x && b[0].x < a[1].x && a[0].y < b[1].y && b[0].y < a[1].y
    };
    let written: Vec<&Text> = laid.texts[from.min(laid.texts.len())..]
        .iter()
        .filter(|t| !t.preview && !t.text.trim().is_empty())
        .collect();
    let texts: Vec<(String, [Point2; 2])> = written
        .iter()
        .map(|t| (t.text.clone(), shrink(notes::text_box(t), 0.3)))
        .collect();
    let corners: Vec<[Point2; 4]> = written
        .iter()
        .map(|t| notes::text_corners(t, 0.3))
        .collect();
    let mut said = Vec::new();
    for (k, (a, _)) in texts.iter().enumerate() {
        for (j, (b, _)) in texts.iter().enumerate().skip(k + 1) {
            if notes::overlap(&corners[k], &corners[j]) {
                said.push(format!(
                    "the texts \"{a}\" and \"{b}\" are written over each other: move one"
                ));
            }
        }
    }
    // (The title block: the box around its cells.)
    let block = laid.cells.iter().fold(None, |b: Option<[Point2; 2]>, c| {
        Some(match b {
            Some([lo, hi]) => [lo.min(c.rect[0]), hi.max(c.rect[1])],
            None => c.rect,
        })
    });
    if let Some(block) = block {
        for (text, rect) in &texts {
            if meet(rect, &block) {
                said.push(format!(
                    "the text \"{text}\" stands on the title block: move it"
                ));
            }
        }
        for view in &laid.views {
            let rect = shrink([view.centre - view.half, view.centre + view.half], 0.5);
            if meet(&rect, &block) {
                said.push(format!(
                    "the view {} reaches onto the title block: move it",
                    view.id
                ));
            }
        }
    }
    said.dedup();
    laid.problems.extend(said);
}

/// What the view `id` of `sheet` is called.
/// What the model `subject` says of itself, and where `note` of it
/// would stand by itself in the view `view` of `sheet`: in millimetres
/// from the view. With no view given, in the first that sees it true —
/// of a datum or a tolerance, one that sees its face edge on, if there
/// is one. `None` if no view does, or there is no such note.
pub fn place(
    sheet: &Sheet,
    subject: &Subject<'_>,
    kept: &Kept,
    note: &str,
    view: Option<&str>,
) -> Option<(String, [f64; 2])> {
    use workpiece_core::world::Note;
    let said = subject.world.said(note)?;
    let said = &*said;
    let frames = views::frames(sheet, subject, kept, &mut Vec::new());
    let fitting = || frames.iter().filter(|placed| notes::fits(said, *placed));
    let placed = match view {
        Some(id) => frames
            .iter()
            .find(|placed| placed.view.id == id && notes::sees(said, *placed))?,
        None => {
            let edge_on = |placed: &&views::Placed<'_>| match &said.note {
                Note::Datum { out, .. } | Note::Geometric { out, .. } => {
                    placed.way(*out).length() > 0.5
                }
                Note::Hole { axis, outside, .. } => (placed.way(*axis).length() < 1e-6) != *outside,
                Note::Dimension { .. } => true,
            };
            fitting().find(edge_on).or_else(|| fitting().next())?
        }
    };
    // Where it shows while it is missing: clear of what stands there.
    let id = placed.view.id.clone();
    let laid = lay_out(sheet, subject, kept, Some(Some(&id)));
    let at = laid
        .notes
        .iter()
        .find_map(|n| n.preview.as_ref().filter(|p| p.0 == note).map(|p| p.1))?;
    Some((id, at))
}

/// A note of the model to be put on a sheet by itself: see
/// [`place_all`].
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    /// Its name in the model.
    pub note: String,
    /// The view it is asked to stand in, if one is.
    pub view: Option<String>,
    /// Where its text is asked to stand, in millimetres from the view's
    /// middle, if it is.
    pub at: Option<[f64; 2]>,
}

/// Where each of `wanted` stands on `sheet`, put one after the other,
/// each clear of what stands on the sheet already (in any view), of
/// those put before it, and of the other views: the view (the one
/// asked for if that sees it true, else the first that does — of a
/// datum or a tolerance, one that sees its face edge on), where its
/// text stands in millimetres from that view's middle, and whether
/// that view is the one asked for. Nothing where no view sees it.
pub fn place_all(
    sheet: &Sheet,
    subject: &Subject<'_>,
    kept: &Kept,
    wanted: &[Wanted],
) -> Vec<Option<(String, [f64; 2], bool)>> {
    workpiece_core::world::annotation::written(sheet.numbers, || {
        let frames = views::frames(sheet, subject, kept, &mut Vec::new());
        notes::place_all(sheet, subject, &frames, wanted)
    })
}

/// How the view `id` of `sheet` looks at the model: what is to its
/// right, what is up in it and what points at who looks; and the point
/// of the model its own coordinates are from.
pub fn looking(
    sheet: &Sheet,
    subject: &Subject<'_>,
    kept: &Kept,
    id: &str,
) -> Option<(
    [workpiece_core::math::Point3; 3],
    workpiece_core::math::Point3,
)> {
    let frames = views::frames(sheet, subject, kept, &mut Vec::new());
    let placed = frames.iter().find(|placed| placed.view.id == id)?;
    Some((placed.basis, placed.middle))
}

/// The letter an auxiliary view is marked with: the views that are
/// not sections or details take theirs from the end of the alphabet.
pub(crate) fn view_mark(sheet: &Sheet, id: &str) -> String {
    let nth = sheet
        .views
        .iter()
        .filter(|v| matches!(v.kind, ViewKind::Auxiliary { .. }))
        .position(|v| v.id == id)
        .unwrap_or(0);
    ["X", "Y", "Z", "W", "V", "U"][nth % 6].to_owned()
}

pub(crate) fn view_label(sheet: &Sheet, id: &str) -> String {
    match sheet.view(id).map(|v| &v.kind) {
        Some(ViewKind::Base { side }) => side.label().to_owned(),
        Some(ViewKind::Named { view }) => view.clone(),
        Some(ViewKind::Projected { of, beside }) => {
            format!("{beside:?} of {}", view_label(sheet, of).to_lowercase())
        }
        Some(ViewKind::Section { letter, .. }) => format!("Section {letter}–{letter}"),
        Some(ViewKind::Auxiliary { of, .. }) => {
            format!("Auxiliary of {}", view_label(sheet, of).to_lowercase())
        }
        Some(ViewKind::Detail { letter, .. }) => format!("Detail {letter}"),
        None => id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A length of 40 along x measured between (0, 0) and (40, 0), its
    /// dimension line 12 above.
    fn length() -> LaidLinear {
        let p = Point2::new;
        LaidLinear {
            from: [p(0.0, 0.0), p(40.0, 0.0)],
            ways: [Point2::Y, Point2::Y],
            line: [p(0.0, 12.0), p(40.0, 12.0)],
            room: 6.0,
        }
    }

    #[test]
    fn strokes_that_meet_are_one_run_whichever_way_they_go() {
        let p = Point2::new;
        let stroke = |a, b| Stroke {
            a,
            b,
            pen: Pen::Dashed,
            preview: false,
        };
        // A curve of three pieces, out of order and one the other way
        // round; and one apart.
        let strokes = [
            stroke(p(1.0, 0.0), p(2.0, 1.0)),
            stroke(p(1.0, 0.0), p(0.0, 0.0)),
            stroke(p(3.0, 1.0), p(2.0, 1.0)),
            stroke(p(9.0, 9.0), p(9.0, 8.0)),
        ];
        let runs = chains(strokes.iter());
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert_eq!(runs[0].len(), 4, "{runs:?}");
        let ends = [runs[0][0], runs[0][3]];
        assert!(
            ends.contains(&p(0.0, 0.0)) && ends.contains(&p(3.0, 1.0)),
            "{runs:?}"
        );
    }

    #[test]
    fn a_dashed_line_runs_on_straight_where_another_branches_off_it() {
        let p = Point2::new;
        let stroke = |a, b| Stroke {
            a,
            b,
            pen: Pen::Dashed,
            preview: false,
        };
        // An upright line in two pieces, its short top piece first, and
        // where they meet a level one going off sideways: the upright
        // line is one run (else its short piece, a run by itself, would
        // be one dash: drawn solid).
        let strokes = [
            stroke(p(0.0, 10.0), p(0.0, 9.0)),
            stroke(p(0.0, 9.0), p(5.0, 9.0)),
            stroke(p(0.0, 9.0), p(0.0, 0.0)),
        ];
        let runs = chains(strokes.iter());
        assert!(
            runs.iter()
                .any(|r| r.contains(&p(0.0, 10.0)) && r.contains(&p(0.0, 0.0))),
            "{runs:?}"
        );
    }

    #[test]
    fn extension_lines_start_off_the_outline_or_on_a_line() {
        let l = length();
        let lines = [(Point2::new(-5.0, 4.0), Point2::new(5.0, 4.0))];
        // Near the sheet's gap; near the line it crosses; else free.
        assert_eq!(l.start_at(0, Point2::new(0.3, 1.4), 1.0, &lines, 1.0), 1.0);
        assert_eq!(l.start_at(0, Point2::new(0.2, 4.6), 1.0, &lines, 1.0), 4.0);
        assert_eq!(l.start_at(0, Point2::new(0.0, 7.0), 1.0, &lines, 1.0), 7.0);
        // (Not past its dimension line.)
        assert_eq!(
            l.start_at(1, Point2::new(40.0, 30.0), 1.0, &lines, 1.0),
            12.0
        );
    }

    #[test]
    fn dimension_texts_snap_to_the_line_and_stack_evenly() {
        let l = length();
        let p = Point2::new;
        // Along the line: its middle, and its ends (the text inside).
        assert_eq!(l.text_at(p(19.0, 30.0), 8.0, &[], 1.5).x, 20.0);
        assert_eq!(l.text_at(p(4.5, 30.0), 8.0, &[], 1.5).x, 4.0);
        assert_eq!(l.text_at(p(35.0, 30.0), 8.0, &[], 1.5).x, 36.0);
        // Up: a pitch off the outline, or two.
        assert_eq!(l.text_at(p(11.0, 8.6), 8.0, &[], 1.5).y, 8.0);
        assert_eq!(l.text_at(p(11.0, 15.2), 8.0, &[], 1.5).y, 16.0);
        // A pitch over another length of the view that runs the same way.
        let other = LaidLinear {
            line: [p(-10.0, 21.0), p(60.0, 21.0)],
            ..length()
        };
        assert_eq!(l.text_at(p(11.0, 28.5), 8.0, &[other], 1.5).y, 29.0);
    }

    #[test]
    fn a_short_centre_line_starts_and_ends_with_long_dashes() {
        // An axis 8 mm long, narrow (0.25): a long dash is 6, so drawn
        // as it is, it would be one long dash and a scrap.
        let dashes = Pen::Chain.fitted_dashes(0.5, 8.0);
        let between: f64 = dashes[1..].iter().sum();
        // Long dashes, gaps and dots that come out to its length: a long
        // dash at each end, a dot between.
        let n = ((8.0 + between) / (dashes[0] + between)).round();
        assert!(n >= 2.0, "{dashes:?}");
        assert!(
            (n * dashes[0] + (n - 1.0) * between - 8.0).abs() < 1e-9,
            "{dashes:?}"
        );
        // A dashed line is as it is.
        assert_eq!(Pen::Dashed.fitted_dashes(0.5, 8.0), Pen::Dashed.dashes(0.5));
    }

    #[test]
    fn a_line_drawn_twice_is_one_run() {
        let p = Point2::new;
        let stroke = |a, b| Stroke {
            a,
            b,
            pen: Pen::Chain,
            preview: false,
        };
        let strokes = [
            stroke(p(0.0, 0.0), p(10.0, 0.0)),
            stroke(p(10.0, 0.0), p(0.0, 0.0)),
        ];
        let runs = chains(strokes.iter());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len(), 2, "{runs:?}");
    }
}
