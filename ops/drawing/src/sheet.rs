//! Drawing sheets, as a drawing step keeps them: the sheet and its
//! title block, the views on it, and which of what is said of the
//! model (its dimensions, datums and tolerances) stands where on it.
//! What a view shows is worked out from the model whenever it is drawn.

use crate::template::{Cell, Template};

/// A sheet size of the A series (ISO 216), trimmed (ISO 5457).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[allow(missing_docs)]
pub enum Format {
    A4,
    #[default]
    A3,
    A2,
    A1,
    A0,
}

impl Format {
    /// Every format, smallest first.
    pub const ALL: [Format; 5] = [Format::A4, Format::A3, Format::A2, Format::A1, Format::A0];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Format::A4 => "A4",
            Format::A3 => "A3",
            Format::A2 => "A2",
            Format::A1 => "A1",
            Format::A0 => "A0",
        }
    }

    /// Its long and its short side, in millimetres.
    pub fn sides(self) -> (f64, f64) {
        match self {
            Format::A4 => (297.0, 210.0),
            Format::A3 => (420.0, 297.0),
            Format::A2 => (594.0, 420.0),
            Format::A1 => (841.0, 594.0),
            Format::A0 => (1189.0, 841.0),
        }
    }
}

/// How the views are arranged about each other (ISO 5456-2).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Projection {
    /// First angle: the view from the left stands to the right, the
    /// view from above below. What ISO drawings use.
    #[default]
    FirstAngle,
    /// Third angle: each view stands on the side it is seen from.
    ThirdAngle,
}

/// How a view is drawn.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Style {
    /// The edges in view; what is hidden is left out.
    #[default]
    HiddenRemoved,
    /// The edges in view, and those hidden dashed.
    HiddenDashed,
    /// Every edge, hidden or not, the same.
    Wireframe,
    /// The faces in their colours under the edges in view.
    Shaded,
}

impl Style {
    /// Every style.
    pub const ALL: [Style; 4] = [
        Style::HiddenRemoved,
        Style::HiddenDashed,
        Style::Wireframe,
        Style::Shaded,
    ];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Style::HiddenRemoved => "Hidden lines removed",
            Style::HiddenDashed => "Hidden lines dashed",
            Style::Wireframe => "Wireframe",
            Style::Shaded => "Shaded",
        }
    }
}

/// How a sheet draws smooth edges: where a round goes on into the face
/// it rounds without a kink (a tangent edge).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum TangentEdges {
    /// Thin, as dimension lines are: they show where the round starts
    /// without reading as an edge.
    #[default]
    Thin,
    /// Not at all.
    Hidden,
    /// As every other edge, wide.
    Wide,
}

/// The side a view sees the model from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[allow(missing_docs)]
pub enum Side {
    #[default]
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    /// From the front, the right and above at once, all three alike
    /// (ISO 5456-3).
    Isometric,
    /// Isometric, from the front, the left and above.
    IsometricFrontLeft,
    /// Isometric, from the back, the right and above.
    IsometricBackRight,
    /// Isometric, from the back, the left and above.
    IsometricBackLeft,
    /// Dimetric (ISO 5456-3): from the front, the right and above, the
    /// depth seen half as long as the width and the height.
    Dimetric,
    /// Isometric, from the front, the right and below.
    IsometricBelowFrontRight,
    /// Isometric, from the front, the left and below.
    IsometricBelowFrontLeft,
    /// Isometric, from the back, the right and below.
    IsometricBelowBackRight,
    /// Isometric, from the back, the left and below.
    IsometricBelowBackLeft,
}

impl Side {
    /// Every side.
    pub const ALL: [Side; 15] = [
        Side::Front,
        Side::Back,
        Side::Left,
        Side::Right,
        Side::Top,
        Side::Bottom,
        Side::Isometric,
        Side::IsometricFrontLeft,
        Side::IsometricBackRight,
        Side::IsometricBackLeft,
        Side::Dimetric,
        Side::IsometricBelowFrontRight,
        Side::IsometricBelowFrontLeft,
        Side::IsometricBelowBackRight,
        Side::IsometricBelowBackLeft,
    ];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Side::Front => "Front",
            Side::Back => "Back",
            Side::Left => "Left",
            Side::Right => "Right",
            Side::Top => "Top",
            Side::Bottom => "Bottom",
            Side::Isometric => "Isometric",
            Side::IsometricFrontLeft => "Isometric front left",
            Side::IsometricBackRight => "Isometric back right",
            Side::IsometricBackLeft => "Isometric back left",
            Side::Dimetric => "Dimetric",
            Side::IsometricBelowFrontRight => "Isometric from below, front right",
            Side::IsometricBelowFrontLeft => "Isometric from below, front left",
            Side::IsometricBelowBackRight => "Isometric from below, back right",
            Side::IsometricBelowBackLeft => "Isometric from below, back left",
        }
    }

    /// How it looks at the model: what is to the right, what is up,
    /// and what points at who looks.
    pub fn basis(self) -> [workpiece_core::math::Point3; 3] {
        use workpiece_core::math::Point3;
        let (up, towards) = match self {
            Side::Front => (Point3::Z, Point3::X),
            Side::Back => (Point3::Z, -Point3::X),
            Side::Right => (Point3::Z, Point3::Y),
            Side::Left => (Point3::Z, -Point3::Y),
            Side::Top => (-Point3::X, Point3::Z),
            Side::Bottom => (Point3::X, -Point3::Z),
            // (From a corner: up is the model's up as it is seen.)
            corner => {
                let towards = match corner {
                    Side::IsometricFrontLeft => Point3::new(1.0, -1.0, 1.0),
                    Side::IsometricBackRight => Point3::new(-1.0, 1.0, 1.0),
                    Side::IsometricBackLeft => Point3::new(-1.0, -1.0, 1.0),
                    Side::IsometricBelowFrontRight => Point3::new(1.0, 1.0, -1.0),
                    Side::IsometricBelowFrontLeft => Point3::new(1.0, -1.0, -1.0),
                    Side::IsometricBelowBackRight => Point3::new(-1.0, 1.0, -1.0),
                    Side::IsometricBelowBackLeft => Point3::new(-1.0, -1.0, -1.0),
                    // (Seen along this, x is foreshortened to a half of
                    // what y and z are: sqrt(1 - 7/9) = 1/2 sqrt(1 - 1/9).)
                    Side::Dimetric => Point3::new(7.0_f64.sqrt(), 1.0, 1.0),
                    _ => Point3::new(1.0, 1.0, 1.0),
                }
                .normalize();
                let up = (Point3::Z - towards * towards.z).normalize();
                (up, towards)
            }
        };
        [up.cross(towards), up, towards]
    }
}

/// Where a projected view stands from the view it is projected from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[allow(missing_docs)]
pub enum Beside {
    Left,
    Right,
    Above,
    Below,
}

/// What a view is of.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ViewKind {
    /// The model seen from a side.
    Base {
        /// The side.
        side: Side,
    },
    /// The model as the view `of` sees it, turned a quarter to stand
    /// beside that view, in line with it.
    Projected {
        /// The view it is projected from.
        of: String,
        /// Where it stands from that view.
        beside: Beside,
    },
    /// The model as a camera view of the part looks at it: a step of
    /// the part, by its name.
    Named {
        /// The camera view.
        view: String,
    },
    /// The model cut open as a section of the part cuts it: a step of
    /// the part, by its name. It is seen from the side that is taken
    /// away, square to the first piece of the line it cuts along. The
    /// line shows, with its arrows and letters, in the view `of` (one
    /// that looks along the cut), and the section keeps in line with
    /// that view where it can.
    Section {
        /// The view the cutting line shows in, if any.
        #[serde(default)]
        of: Option<String>,
        /// The section.
        section: String,
        /// Its letter: the view is called `A–A`.
        letter: String,
        /// How far it is turned on the sheet, counter-clockwise, in
        /// degrees: a section turned to stand as the drawing wants it
        /// stands where it is put, in line with nothing.
        #[serde(default)]
        turn: f64,
    },
    /// The model seen along the sheet, square to a direction of the
    /// view `of`: what a face that lies askew in that view looks like
    /// seen true (an auxiliary view).
    Auxiliary {
        /// The view it is taken from.
        of: String,
        /// The direction it looks along in that view, as an angle from
        /// that view's right, counter-clockwise, in degrees.
        angle: f64,
    },
    /// A part of the view `of`, larger.
    Detail {
        /// The view it is a part of.
        of: String,
        /// The middle of the part, in that view's own coordinates (the
        /// model's lengths, from the middle of what the view shows).
        centre: [f64; 2],
        /// How far around the middle it reaches, in the model's
        /// lengths.
        radius: f64,
        /// Its letter.
        letter: String,
        /// Where its letter stands by the circle in the view it is of:
        /// its middle, in millimetres on the sheet from the circle's
        /// middle. Where it is not said, by itself: outside the circle,
        /// clear of what is drawn and written there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        letter_at: Option<[f64; 2]>,
    },
}

/// A view on a sheet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SheetView {
    /// What it is called on the sheet's own account: other views and
    /// what stands in it refer to it by this.
    pub id: String,
    /// What it is of.
    pub kind: ViewKind,
    /// Where its middle is on the sheet, in millimetres from the
    /// sheet's lower left corner. (A projected view keeps in line with
    /// the view it is projected from.)
    pub at: [f64; 2],
    /// Its scale, as two whole numbers (`[2, 1]` is 2:1), where it is
    /// not the sheet's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[u32; 2]>,
    /// How it is drawn.
    #[serde(default)]
    pub style: Style,
    /// The bodies it shows, by name; all of them, if none is named.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<String>,
    /// Of a section: whether only what the cut meets is drawn — the
    /// cut faces, hatched — and nothing of what lies behind it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut_only: bool,
    /// Of a section: whether it is a half section (ISO 128-3) — cut on
    /// one side of its middle line only, the other half drawn as the
    /// model is seen from there uncut (or not at all), the two divided
    /// by a centre line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub half: Option<Half>,
    /// Of a view from a side (an isometric one too): the section of the
    /// part, by its name, that cuts the model open in it — what it takes
    /// away left out, its cut hatched (a pictorial section).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Where its caption (`B–B`, `A (5:1)`, a scale of its own) stands
    /// where it is put by hand: its middle, in millimetres on the sheet
    /// from the view's own middle (it moves with the view). Where it is
    /// not said, by itself: over the view (a scale, under it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption_at: Option<[f64; 2]>,
}

/// The half of a view, either side of its middle line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[allow(missing_docs)]
pub enum HalfSide {
    Left,
    Right,
    Above,
    Below,
}

impl HalfSide {
    /// Every half.
    pub const ALL: [HalfSide; 4] = [
        HalfSide::Left,
        HalfSide::Right,
        HalfSide::Above,
        HalfSide::Below,
    ];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            HalfSide::Left => "Left",
            HalfSide::Right => "Right",
            HalfSide::Above => "Above",
            HalfSide::Below => "Below",
        }
    }

    /// Whether the point `p` of a view (from its middle line) is in
    /// this half.
    pub fn holds(self, p: workpiece_core::math::Point2) -> bool {
        match self {
            HalfSide::Left => p.x <= 0.0,
            HalfSide::Right => p.x >= 0.0,
            HalfSide::Above => p.y >= 0.0,
            HalfSide::Below => p.y <= 0.0,
        }
    }
}

/// How a section view is cut in half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Half {
    /// The half that is shown cut.
    pub cut: HalfSide,
    /// Whether only that half is drawn — the section from the middle
    /// line out, nothing of the other half; else the other half is
    /// drawn as the view sees the model uncut.
    #[serde(default)]
    pub only: bool,
}

/// Something said of the model, placed in a view.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlacedNote {
    /// The annotation's name in the model: `sketch1/d4`, `dimension2`.
    pub note: String,
    /// The view it stands in.
    pub view: String,
    /// Where its text stands on the sheet, in millimetres from the
    /// view's middle.
    pub at: [f64; 2],
    /// How many decimals its number has, if not as the sheet's have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    /// Where each of its extension lines starts, in millimetres on the
    /// sheet from what it measures (the outline), if not where the
    /// sheet's [`Spacing::gap`] has it.
    #[serde(default, skip_serializing_if = "no_starts")]
    pub starts: [Option<f64>; 2],
    /// Of a hole's callout, how many holes alike it says it is of, if
    /// not as the model has it (its pattern, its mirror image): `0` says
    /// no count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

fn no_starts(starts: &[Option<f64>; 2]) -> bool {
    starts.iter().all(Option::is_none)
}

/// How a sheet's dimensions stand (ISO 129-1): how far an extension
/// line starts off the outline, and how far apart dimension lines are
/// stacked — what dragging them snaps to.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Spacing {
    /// The gap between the outline and where an extension line starts,
    /// in millimetres.
    pub gap: f64,
    /// How far apart dimension lines stacked over each other are, and
    /// the first from the outline, in millimetres.
    pub pitch: f64,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            gap: 1.0,
            pitch: 8.0,
        }
    }
}

/// A dimension the sheet has of its own: between two points of a view,
/// measured in the view.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OwnDimension {
    /// The view it is in.
    pub view: String,
    /// From here, in the view's own coordinates (the model's lengths,
    /// from the middle of what the view shows).
    pub from: [f64; 2],
    /// To here.
    pub to: [f64; 2],
    /// Along what it measures: across (`Some(false)`), up
    /// (`Some(true)`), or straight from the one to the other (`None`).
    #[serde(default)]
    pub upright: Option<bool>,
    /// Whether it is of something round: `from` is its middle and `to`
    /// a point of it, and what is said is its diameter.
    #[serde(default)]
    pub round: bool,
    /// Where its text stands on the sheet, in millimetres from the
    /// view's middle.
    pub at: [f64; 2],
}

/// A drawing sheet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Sheet {
    /// What it is called.
    pub name: String,
    /// Its size.
    pub format: Format,
    /// Whether it stands upright (A4 mostly does); else it lies on its
    /// long side.
    pub upright: bool,
    /// The scale of its views (`[1, 2]` is 1:2).
    pub scale: [u32; 2],
    /// How the views are arranged about each other.
    pub projection: Projection,
    /// The template it was made from, by name.
    pub template: String,
    /// The title block: its cells, with what they say.
    pub block: Vec<Cell>,
    /// The views.
    pub views: Vec<SheetView>,
    /// How its numbers are written, if not as ISO 129-1 has them
    /// (a comma, no more decimals than needed): inch drawings write
    /// `.750`.
    #[serde(default)]
    pub numbers: workpiece_core::world::Numbers,
    /// What is said of the model, where it stands on the sheet.
    pub notes: Vec<PlacedNote>,
    /// The dimensions that are the sheet's own.
    pub own: Vec<OwnDimension>,
    /// What is drawn on the sheet by hand — lines, circles, arcs,
    /// lettering, with their constraints and dimensions — as a sketch,
    /// in millimetres from the sheet's lower left corner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drawn: Option<workpiece_ops_sketch::SketchArgs>,
    /// Words written on the sheet beside a view: `(2) PLACES`, `THRU`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Words>,
    /// How its dimensions stand off the outline and one another.
    pub spacing: Spacing,
    /// How it draws smooth (tangent) edges.
    #[serde(default)]
    pub tangent_edges: TangentEdges,
    /// The centre lines and datum traces put on it (ISO 128-2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<SheetLine>,
    /// The centre lines it draws of what is round without their being
    /// put on it one by one.
    #[serde(default)]
    pub centres: Centres,
    /// Its line widths, lettering and centre lines: the ISO values.
    #[serde(default)]
    pub drafting: Drafting,
    /// Whether it has a parts list (ISO 7573) over its title block: of
    /// the parts placed in the model, each kind of them an item with
    /// its number, how many, its name and its material.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parts_list: bool,
    /// The columns of the parts list, in order; none: item, quantity,
    /// name and material.
    #[serde(default)]
    pub parts_columns: Vec<PartsColumn>,
    /// Whether every item of the parts list has a balloon (ISO 6433)
    /// put by itself, where `balloons` puts none.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_balloons: bool,
    /// The balloons put by hand (or dragged): where each stands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balloons: Vec<Balloon>,
    /// Whether a hole's callout says `THRU` of a hole that goes through
    /// (as drawings to ASME Y14.5 do); else, as ISO 129-1 has it, a
    /// callout without a depth is of a hole through.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub thru: bool,
    /// Whether the text of a radius or a diameter on a leader runs
    /// along the leader (aligned); else it stands level on a short
    /// shoulder at the leader's end (ISO 129-1), as most drawings have
    /// it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aligned_leaders: bool,
    /// Whether a hole's callout writes how many holes alike it is of as
    /// `(4) PLACES` after it, as some drawings do; else `4×` before it
    /// (ISO 129-1).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub places: bool,
    /// Whether a hole's callout writes a depth that follows from a face
    /// (a bore or a counterbore up to a plane) as a reference, in
    /// brackets: `↧(50)`; else it leaves it out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reference_depths: bool,
}

/// A column of a parts list (ISO 7573).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PartsColumn {
    /// The item's number.
    Item,
    /// How many of it there are.
    Qty,
    /// What it is called.
    Name,
    /// What it is made of.
    Material,
    /// How dense that is, kg/m³.
    Density,
}

impl PartsColumn {
    /// The columns a parts list has where nothing else is said.
    pub const USUAL: [PartsColumn; 4] = [
        PartsColumn::Item,
        PartsColumn::Qty,
        PartsColumn::Name,
        PartsColumn::Material,
    ];

    /// What its head says.
    pub fn head(self) -> &'static str {
        match self {
            PartsColumn::Item => "Item",
            PartsColumn::Qty => "Qty",
            PartsColumn::Name => "Name",
            PartsColumn::Material => "Material",
            PartsColumn::Density => "Density",
        }
    }
}

/// An item reference (ISO 6433): the item's number in a circle, on a
/// narrow leader that ends in a dot on the part.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Balloon {
    /// The part it points at: the name it is placed under (any of an
    /// item's names stands for the item).
    pub part: String,
    /// The view it stands in.
    pub view: String,
    /// The middle of its circle, in millimetres from the view's middle.
    pub at: [f64; 2],
    /// Where its leader ends, in millimetres from the view's middle;
    /// none: on the part, where the view shows it, by itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<[f64; 2]>,
}

/// The line widths, dashes, lettering and arrowheads of a sheet, as
/// ISO 128-2, ISO 129-1 and ISO 3098 have them: kept in one place —
/// the template and the sheet made from it — so that what is drawn on
/// screen, written to PDF, SVG or DXF, and made by the command line
/// agrees.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Drafting {
    /// The width of the wide lines, in millimetres, from the series of
    /// ISO 128-2 (0.13, 0.18, 0.25, 0.35, 0.5, 0.7, 1, 1.4, 2); the
    /// narrow lines are half as wide. None: as the format has it — 0.5
    /// on A4 to A2, 0.7 on A1 and A0.
    pub wide: Option<f64>,
    /// How far centre lines and the traces of datums run on past the
    /// outline of what they are of, in millimetres on the sheet.
    pub past: f64,
    /// Of circles smaller across than this on the sheet (millimetres),
    /// the centre lines are drawn continuous narrow: dashes would not
    /// show on them.
    pub small: f64,
    /// The height of the lettering of dimensions and notes (ISO 3098:
    /// 2.5, 3.5, 5, 7).
    pub letter: f64,
}

impl Default for Drafting {
    fn default() -> Self {
        Self {
            wide: None,
            past: 3.0,
            small: 12.0,
            letter: 3.5,
        }
    }
}

impl Drafting {
    /// The width of the wide lines on a sheet of `format`.
    pub fn wide(&self, format: Format) -> f64 {
        self.wide.unwrap_or(match format {
            Format::A0 | Format::A1 => 0.7,
            _ => 0.5,
        })
    }
}

/// A centre line or the trace of a datum, put in a view: long-dashed
/// dotted narrow (ISO 128-2, type 04.1), a little past what it is of.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SheetLine {
    /// The view it is in; none: every view that shows it.
    #[serde(default)]
    pub view: Option<String>,
    /// What it is of.
    pub of: LineOf,
    /// How much farther than the sheet's [`Drafting::past`] each of its
    /// ends runs on (less, if negative), in millimetres on the sheet.
    #[serde(default, skip_serializing_if = "no_ends")]
    pub ends: [f64; 2],
    /// Whether it is not drawn: a centre line the sheet would draw by
    /// itself (of a hole), taken off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Where it is broken: stretches of it not drawn, each from and to
    /// in millimetres on the sheet along it from its start (a centre
    /// line across a gap between two lugs, drawn only over them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<[f64; 2]>,
    /// Whether it is marked as a line of symmetry (ISO 128-2): two short
    /// parallel narrow lines square across it at each end.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub symmetry: bool,
    /// Words written on it at its end: `CL SYM`, `℄`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Where its words stand: at its start, its end, or so many
    /// millimetres along it from its start; none: at whichever end they
    /// stand clear of what is written there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_at: Option<LabelAt>,
}

/// Where the words of a line stand on it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum LabelAt {
    /// At one of its ends.
    End(LineEnd),
    /// So many millimetres along it from its start.
    Along(f64),
}

/// An end of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnd {
    /// Where it starts.
    Start,
    /// Where it ends.
    End,
}

fn no_ends(ends: &[f64; 2]) -> bool {
    ends.iter().all(|e| *e == 0.0)
}

/// What a line of a sheet is of: it follows that in the model.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum LineOf {
    /// A datum of the model, by name: a plane seen edge on (its trace),
    /// an axis, a frame's origin (a cross); `frame/x`, `frame/y`,
    /// `frame/z` for the axes of a frame.
    Datum(String),
    /// The axis of something round of the model — a bore, a boss, a
    /// turned body: by the name of one of its round faces, or of the
    /// step that made it (its faces' names start with it).
    Axis(String),
    /// The circle through the middles of holes about one middle (a
    /// bolt circle), by the name of one of the holes' faces or of the
    /// step that made them.
    Pitch(String),
    /// The path a sweep runs along (a swept pipe's centre line), by the
    /// name of its sketch: drawn where a view sees the sketch's plane
    /// square on.
    Path(String),
}

impl LineOf {
    /// What it is called in a list.
    pub fn label(&self) -> String {
        match self {
            LineOf::Datum(name) => name.clone(),
            LineOf::Axis(name) => format!("axis of {name}"),
            LineOf::Pitch(name) => format!("pitch circle of {name}"),
            LineOf::Path(name) => format!("path of {name}"),
        }
    }
}

/// Which centre lines a sheet draws by itself, without their being put
/// on it one by one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Centres {
    /// Those of everything round: bores, bosses, turned bodies.
    #[default]
    All,
    /// Those of holes only.
    Holes,
    /// None.
    None,
}

impl Centres {
    /// Every choice.
    pub const ALL: [Centres; 3] = [Centres::All, Centres::Holes, Centres::None];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Centres::All => "Of everything round",
            Centres::Holes => "Of holes",
            Centres::None => "Only those put on",
        }
    }
}

/// Where the leader of words points: a point of the model, or a point
/// of their view.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Target {
    /// A point of the model, in its lengths: the leader follows it.
    Model([f64; 3]),
    /// A point of the view, in its own coordinates (the model's lengths
    /// from the middle of what the view shows, across and up).
    View([f64; 2]),
}

/// Words written on a sheet: a remark beside a view.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Words {
    /// The view they stand by (they move with it); none for words that
    /// stand on the sheet itself.
    #[serde(default)]
    pub view: Option<String>,
    /// Where their middle is, in millimetres from the view's middle
    /// (or from the sheet's lower left corner); of words that belong to
    /// a dimension, how far they are moved off where they stand by it.
    pub at: [f64; 2],
    /// The dimension they belong to, by the name of what is said of the
    /// model (`corners`, `sketch1/d4`; a step's name stands for the
    /// first it says): they stand under its text, turned as it is, and
    /// go where it goes (`R32` over `(4) PLACES`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    /// What they say.
    pub text: String,
    /// How high their capitals are, in millimetres.
    #[serde(default = "letter")]
    pub height: f64,
    /// Where their leaders point, if they have any (words of a view):
    /// one from the words to each; each ends in an arrowhead on an
    /// outline, in a dot within a face (ISO 128-22).
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "one_or_more"
    )]
    pub to: Vec<Target>,
}

fn letter() -> f64 {
    3.5
}

/// Leaders said as one place (as sheets had them), none, or a list.
fn one_or_more<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Target>, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Said {
        One(Target),
        More(Vec<Target>),
    }
    Ok(
        match <Option<Said> as serde::Deserialize>::deserialize(d)? {
            None => Vec::new(),
            Some(Said::One(t)) => vec![t],
            Some(Said::More(ts)) => ts,
        },
    )
}

impl Default for Sheet {
    fn default() -> Self {
        Self {
            name: "Sheet 1".into(),
            format: Format::A3,
            upright: false,
            scale: [1, 1],
            projection: Projection::FirstAngle,
            template: String::new(),
            block: crate::template::iso_block(),
            views: Vec::new(),
            numbers: Default::default(),
            notes: Vec::new(),
            own: Vec::new(),
            drawn: None,
            words: Vec::new(),
            spacing: Spacing::default(),
            tangent_edges: TangentEdges::default(),
            lines: Vec::new(),
            centres: Centres::default(),
            drafting: Drafting::default(),
            parts_list: false,
            parts_columns: Vec::new(),
            auto_balloons: false,
            balloons: Vec::new(),
            thru: false,
            aligned_leaders: false,
            places: false,
            reference_depths: false,
        }
    }
}

/// The scales a drawing uses (ISO 5455): reductions, full size,
/// enlargements.
pub const SCALES: [[u32; 2]; 13] = [
    [1, 100],
    [1, 50],
    [1, 20],
    [1, 10],
    [1, 5],
    [1, 2],
    [1, 1],
    [2, 1],
    [5, 1],
    [10, 1],
    [20, 1],
    [50, 1],
    [100, 1],
];

impl Sheet {
    /// What is drawn on it by hand, as a sketch (an empty one, while
    /// nothing is).
    pub fn hand(&self) -> workpiece_ops_sketch::Sketch {
        match &self.drawn {
            Some(args) => workpiece_ops_sketch::Sketch {
                id: "drawn".to_owned(),
                args: args.clone(),
            },
            None => {
                // (Nothing to measure from but what is drawn: no
                // origin, no axes.)
                let mut sketch = workpiece_ops_sketch::Sketch::new("drawn");
                sketch.args.entities.clear();
                sketch.args.points.clear();
                sketch
            }
        }
    }

    /// A sheet called `name` for `world`, whose lengths are in `unit`,
    /// made from `template`: its views at the largest of the usual
    /// scales the model has room at with the views beside each other,
    /// what is projected as far from what it is projected from as the
    /// model is large.
    pub fn of(
        world: &workpiece_core::world::World,
        unit: workpiece_core::Unit,
        name: &str,
        template: &Template,
    ) -> Sheet {
        let mut sheet = Sheet {
            name: name.to_owned(),
            format: template.format,
            upright: template.upright,
            projection: template.projection,
            template: template.name.clone(),
            block: template.block.clone(),
            spacing: template.spacing,
            drafting: template.drafting,
            // (The centre lines of holes by themselves; those of other
            // round things, and datums, are put on as wanted.)
            centres: Centres::Holes,
            // (Inch drawings say THRU of a hole through.)
            thru: unit == workpiece_core::Unit::Inch,
            // (Inch drawings write three decimals after a point, and no
            // zero before it.)
            numbers: match unit {
                workpiece_core::Unit::Inch => workpiece_core::world::Numbers {
                    decimals: Some(3),
                    bare: true,
                    point: true,
                    trim: false,
                },
                _ => Default::default(),
            },
            ..Default::default()
        };
        let [w, h] = sheet.size();
        // How large the model is, in millimetres: what the scale is
        // chosen by, and the views are set apart by.
        let points = world.topo.vertices();
        let mut points = points.iter().map(|(_, v)| v.point);
        let size = match points.next() {
            Some(first) => {
                let (lo, hi) = points.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
                (hi - lo).max(workpiece_core::math::Point3::splat(1e-9)) * unit.millimetres()
            }
            None => workpiece_core::math::Point3::splat(50.0),
        };
        let fit = (w * 0.26 / size.x.max(size.y)).min(h * 0.26 / size.z.max(size.y));
        let scale = SCALES
            .iter()
            .rev()
            .find(|s| f64::from(s[0]) / f64::from(s[1]) <= fit)
            .copied()
            .unwrap_or([1, 100]);
        sheet.scale = scale;
        let k = f64::from(scale[0]) / f64::from(scale[1]);
        let gap = 50.0;
        let aside = (size.x + size.y) / 2.0 * k + gap;
        let below = (size.z + size.y) / 2.0 * k + gap;
        for view in &template.views {
            let mut view = view.clone();
            if let ViewKind::Projected { of, beside } = &view.kind
                && let Some(from) = sheet.view(of)
            {
                let [x, y] = from.at;
                view.at = match beside {
                    Beside::Right => [x + aside, y],
                    Beside::Left => [x - aside, y],
                    Beside::Below => [x, y - below],
                    Beside::Above => [x, y + below],
                };
            }
            sheet.views.push(view);
        }
        sheet
    }

    /// Its width and height, in millimetres.
    pub fn size(&self) -> [f64; 2] {
        let (long, short) = self.format.sides();
        if self.upright {
            [short, long]
        } else {
            [long, short]
        }
    }

    /// The view called `id`.
    pub fn view(&self, id: &str) -> Option<&SheetView> {
        self.views.iter().find(|v| v.id == id)
    }

    /// A name no view of the sheet has yet: `prefix` and a number.
    pub fn fresh_view(&self, prefix: &str) -> String {
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|id| self.view(id).is_none())
            .expect("unbounded range")
    }

    /// The first letter no section or detail of the sheet has yet (but
    /// I, O and Q, which are not used).
    pub fn fresh_letter(&self) -> String {
        let taken: Vec<&str> = self
            .views
            .iter()
            .filter_map(|v| match &v.kind {
                ViewKind::Section { letter, .. } | ViewKind::Detail { letter, .. } => {
                    Some(letter.as_str())
                }
                _ => None,
            })
            .collect();
        ('A'..='Z')
            .filter(|c| !matches!(c, 'I' | 'O' | 'Q'))
            .map(String::from)
            .find(|l| !taken.contains(&l.as_str()))
            .unwrap_or_else(|| "A".into())
    }

    /// The sheet without the view `id`, nor what was projected or cut
    /// from it, nor what stood in those.
    pub fn without_view(&self, id: &str) -> Sheet {
        let mut gone = vec![id.to_owned()];
        loop {
            let more: Vec<String> = self
                .views
                .iter()
                .filter(|v| !gone.contains(&v.id))
                .filter(|v| match &v.kind {
                    ViewKind::Base { .. } | ViewKind::Named { .. } => false,
                    ViewKind::Section { of, .. } => of.as_ref().is_some_and(|of| gone.contains(of)),
                    ViewKind::Projected { of, .. }
                    | ViewKind::Auxiliary { of, .. }
                    | ViewKind::Detail { of, .. } => gone.contains(of),
                })
                .map(|v| v.id.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            gone.extend(more);
        }
        let mut sheet = self.clone();
        sheet.views.retain(|v| !gone.contains(&v.id));
        sheet.notes.retain(|n| !gone.contains(&n.view));
        sheet.own.retain(|d| !gone.contains(&d.view));
        sheet
    }
}
