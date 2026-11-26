//! Annotations: what is said of the model on the model itself — its
//! dimensions with their tolerances, its datum features, its geometric
//! tolerances. A drawing shows them; nothing of the geometry follows
//! from them.

use crate::math::Point3;

/// The tolerance of a dimension.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum Tolerance {
    /// None given: the drawing's general tolerance applies.
    #[default]
    General,
    /// Plus and minus the same amount: `10 ±0,1`.
    Symmetric(f64),
    /// An upper and a lower deviation: `10 +0,2 −0,1`.
    Deviations {
        /// The upper deviation.
        upper: f64,
        /// The lower deviation.
        lower: f64,
    },
    /// An upper and a lower deviation, written as the two limits of
    /// size the one over the other: `10,2` over `9,9`.
    Limits {
        /// The upper deviation.
        upper: f64,
        /// The lower deviation.
        lower: f64,
    },
    /// An ISO 286 tolerance class: `⌀10 H7`.
    Fit(String),
    /// For reference only: the value is written in brackets, `(60)`,
    /// and nothing is held to it — it follows from other dimensions.
    Reference,
}

/// What a dimension measures, where it is in the model.
#[derive(Debug, Clone, PartialEq)]
pub enum Measured {
    /// The distance from one point to another: as they lie, or along a
    /// direction.
    Length {
        /// From here.
        from: Point3,
        /// To here.
        to: Point3,
        /// Along this (a unit vector), if not straight from one to the
        /// other.
        along: Option<Point3>,
    },
    /// How far one round thing is off another about the same axis — a
    /// wall's thickness, an offset: from the one radius to the other,
    /// whichever way across the axis a view shows it.
    Radial {
        /// A point of the axis, where the round things are.
        centre: Point3,
        /// The axis (a unit vector).
        axis: Point3,
        /// The one radius.
        inner: f64,
        /// The other.
        outer: f64,
    },
    /// The diameter of something round about an axis.
    Diameter {
        /// A point of the axis, where the round thing is.
        centre: Point3,
        /// The axis (a unit vector); nothing (zero) for a ball, which
        /// is round every way and is seen so from anywhere.
        axis: Point3,
        /// Its radius.
        radius: f64,
    },
    /// The radius of something round about an axis.
    Radius {
        /// A point of the axis, where the round thing is.
        centre: Point3,
        /// The axis (a unit vector); nothing (zero) for a ball.
        axis: Point3,
        /// Its radius.
        radius: f64,
    },
    /// The angle between two directions at a point.
    Angle {
        /// Where they meet.
        at: Point3,
        /// The one (a unit vector).
        first: Point3,
        /// The other (a unit vector).
        second: Point3,
    },
}

/// What a geometric tolerance is of (ISO 1101).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[allow(missing_docs)]
pub enum Characteristic {
    Straightness,
    #[default]
    Flatness,
    Circularity,
    Cylindricity,
    LineProfile,
    SurfaceProfile,
    Parallelism,
    Perpendicularity,
    Angularity,
    Position,
    Concentricity,
    Symmetry,
    CircularRunout,
    TotalRunout,
}

impl Characteristic {
    /// Every characteristic: of form, of orientation, of location, of
    /// run-out.
    pub const ALL: [Characteristic; 14] = [
        Characteristic::Straightness,
        Characteristic::Flatness,
        Characteristic::Circularity,
        Characteristic::Cylindricity,
        Characteristic::LineProfile,
        Characteristic::SurfaceProfile,
        Characteristic::Parallelism,
        Characteristic::Perpendicularity,
        Characteristic::Angularity,
        Characteristic::Position,
        Characteristic::Concentricity,
        Characteristic::Symmetry,
        Characteristic::CircularRunout,
        Characteristic::TotalRunout,
    ];

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Characteristic::Straightness => "Straightness",
            Characteristic::Flatness => "Flatness",
            Characteristic::Circularity => "Roundness",
            Characteristic::Cylindricity => "Cylindricity",
            Characteristic::LineProfile => "Profile of a line",
            Characteristic::SurfaceProfile => "Profile of a surface",
            Characteristic::Parallelism => "Parallelism",
            Characteristic::Perpendicularity => "Perpendicularity",
            Characteristic::Angularity => "Angularity",
            Characteristic::Position => "Position",
            Characteristic::Concentricity => "Concentricity",
            Characteristic::Symmetry => "Symmetry",
            Characteristic::CircularRunout => "Circular run-out",
            Characteristic::TotalRunout => "Total run-out",
        }
    }

    /// Whether it is of the feature's form alone: it takes no datum.
    pub fn of_form(self) -> bool {
        matches!(
            self,
            Characteristic::Straightness
                | Characteristic::Flatness
                | Characteristic::Circularity
                | Characteristic::Cylindricity
        )
    }
}

/// A piece of a hole's callout (ISO 129-1).
#[derive(Debug, Clone, PartialEq)]
pub enum Callout {
    /// How many holes alike it is of (more than one): `4× ` before it,
    /// or as a sheet may write it, `(4) PLACES` after it.
    Count(usize),
    /// Words as they are: ` THRU`, a tolerance class.
    Text(String),
    /// A number, written as the sheet writes numbers.
    Number(f64),
    /// The depth sign (↧).
    Depth,
    /// The counterbore sign (⌴).
    Counterbore,
    /// The countersink sign (⌵).
    Countersink,
    /// The tolerance of the size before it (` H7`, ` ±0,1`), written as
    /// the sheet writes tolerances.
    Tolerance {
        /// The tolerance.
        tolerance: Tolerance,
        /// The nominal size it is of.
        size: f64,
    },
}

impl Annotation {
    /// Of a hole's callout, it as of `times` as many holes alike (a
    /// pattern, a mirror image of the hole made after it): its count so
    /// many times what it was. Nothing of anything else.
    #[must_use]
    pub fn counted(&self, times: usize) -> Option<Self> {
        let Note::Hole { lines, .. } = &self.note else {
            return None;
        };
        let had = lines
            .first()
            .and_then(|l| {
                l.iter().find_map(|c| match c {
                    Callout::Count(n) => Some(*n),
                    _ => None,
                })
            })
            .unwrap_or(1);
        let mut said = self.clone();
        if let Note::Hole { lines, .. } = &mut said.note {
            let first = lines.first_mut()?;
            first.retain(|c| !matches!(c, Callout::Count(_)));
            if had * times > 1 {
                first.insert(0, Callout::Count(had * times));
            }
        }
        Some(said)
    }
}

impl Callout {
    /// As it is written in plain text.
    pub fn said(&self) -> String {
        match self {
            Callout::Count(n) => format!("{n}× "),
            Callout::Text(text) => text.clone(),
            Callout::Number(x) => number(*x),
            Callout::Depth => "↧".into(),
            Callout::Counterbore => "⌴".into(),
            Callout::Countersink => "⌵".into(),
            Callout::Tolerance { tolerance, size } => tolerance.suffix(*size, ""),
        }
    }
}

/// What an annotation says.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// What a hole or a thread is (ISO 129-1, ISO 6410-2), on one
    /// leader to its rim: a hole's diameter and depth, its counterbore
    /// or countersink, how many there are — `4× Ø6 ↧10` over `⌴Ø11 ↧6`;
    /// a thread's designation — `M16×1.5`.
    Hole {
        /// The middle of its mouth (of the first, where there are
        /// several).
        centre: Point3,
        /// Its axis, into the material (a unit vector).
        axis: Point3,
        /// The radius of its rim, where the leader ends.
        radius: f64,
        /// The lines of the callout, each of pieces.
        lines: Vec<Vec<Callout>>,
        /// Whether it is on the outside of a rod (a screw's thread):
        /// its leader ends on the rod's outline, best seen from the
        /// side.
        outside: bool,
    },
    /// A dimension: what it measures, its value, its tolerance.
    Dimension {
        /// What is measured.
        measured: Measured,
        /// The value: a length, or an angle in degrees.
        value: f64,
        /// Its tolerance.
        tolerance: Tolerance,
        /// The same size measured at other places of the model (along
        /// each edge of a chamfer, where it turns across a view): a
        /// view that does not see `measured` true shows the first of
        /// these it does.
        elsewhere: Vec<Measured>,
    },
    /// A datum feature (ISO 5459): a face the model is measured from,
    /// by its letter.
    Datum {
        /// The letter.
        letter: String,
        /// A point of the feature.
        on: Point3,
        /// The way out of the body there (a unit vector).
        out: Point3,
    },
    /// A geometric tolerance (ISO 1101).
    Geometric {
        /// What is toleranced.
        characteristic: Characteristic,
        /// The width of the tolerance zone.
        value: f64,
        /// Whether the zone is round: the value is its diameter.
        diametral: bool,
        /// The datums it is taken from, in order.
        datums: Vec<String>,
        /// A point of the feature.
        on: Point3,
        /// The way out of the body there (a unit vector).
        out: Point3,
    },
}

/// Something said of the model, where the model is. It shows on the
/// model only where what is said of it is looked at (the Annotate tab,
/// a sketch its own in the sketch), and on the model's drawings.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// What it says.
    pub note: Note,
    /// Where its text stands.
    pub at: Point3,
    /// The sketch it is a dimension of, if it is one: it lies in that
    /// sketch's plane, and shows with the sketch.
    pub sketch: Option<SketchOf>,
}

impl Annotation {
    /// It as it is said of a part placed by `pose`: its points and ways
    /// where the part is.
    #[must_use]
    pub fn placed(&self, pose: crate::Pose) -> Self {
        let p = |q: Point3| pose.transform_point(q);
        let v = |q: Point3| pose.transform_vector(q);
        let measured = |m: &Measured| match m {
            Measured::Length { from, to, along } => Measured::Length {
                from: p(*from),
                to: p(*to),
                along: along.map(v),
            },
            Measured::Radial {
                centre,
                axis,
                inner,
                outer,
            } => Measured::Radial {
                centre: p(*centre),
                axis: v(*axis),
                inner: *inner,
                outer: *outer,
            },
            Measured::Diameter {
                centre,
                axis,
                radius,
            } => Measured::Diameter {
                centre: p(*centre),
                axis: v(*axis),
                radius: *radius,
            },
            Measured::Radius {
                centre,
                axis,
                radius,
            } => Measured::Radius {
                centre: p(*centre),
                axis: v(*axis),
                radius: *radius,
            },
            Measured::Angle { at, first, second } => Measured::Angle {
                at: p(*at),
                first: v(*first),
                second: v(*second),
            },
        };
        let note = match &self.note {
            Note::Hole {
                centre,
                axis,
                radius,
                lines,
                outside,
            } => Note::Hole {
                centre: p(*centre),
                axis: v(*axis),
                radius: *radius,
                lines: lines.clone(),
                outside: *outside,
            },
            Note::Dimension {
                measured: m,
                value,
                tolerance,
                elsewhere,
            } => Note::Dimension {
                measured: measured(m),
                value: *value,
                tolerance: tolerance.clone(),
                elsewhere: elsewhere.iter().map(measured).collect(),
            },
            Note::Datum { letter, on, out } => Note::Datum {
                letter: letter.clone(),
                on: p(*on),
                out: v(*out),
            },
            Note::Geometric {
                characteristic,
                value,
                diametral,
                datums,
                on,
                out,
            } => Note::Geometric {
                characteristic: *characteristic,
                value: *value,
                diametral: *diametral,
                datums: datums.clone(),
                on: p(*on),
                out: v(*out),
            },
        };
        Self {
            note,
            at: p(self.at),
            sketch: self.sketch.as_ref().map(|s| SketchOf {
                id: s.id.clone(),
                constraint: s.constraint,
                geometry: s.geometry.clone(),
                origin: p(s.origin),
                x: v(s.x),
                y: v(s.y),
            }),
        }
    }
}

/// The sketch an annotation is a dimension of, and the plane it lies in.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchOf {
    /// The sketch's name.
    pub id: String,
    /// The sketch's constraint the dimension is.
    pub constraint: u32,
    /// The sketch as it was solved, for the layout of its dimensions (the
    /// sketch operation puts in and reads out what it likes; the
    /// dimension is laid out in the sketch's own coordinates).
    pub geometry: Opaque,
    /// Where the sketch's (0, 0) is.
    pub origin: Point3,
    /// Along the plane, where the sketch's u points.
    pub x: Point3,
    /// Along the plane, where the sketch's v points.
    pub y: Point3,
}

/// Something an operation keeps in the world for itself to read back:
/// shared, equal only to itself.
#[derive(Clone)]
pub struct Opaque(pub std::sync::Arc<dyn std::any::Any + Send + Sync>);

impl Opaque {
    /// `value`, kept.
    pub fn new<T: std::any::Any + Send + Sync>(value: T) -> Self {
        Self(std::sync::Arc::new(value))
    }

    /// What was kept, if it is a `T`.
    pub fn get<T: std::any::Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl std::fmt::Debug for Opaque {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Opaque")
    }
}

impl PartialEq for Opaque {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

/// How numbers are written: a sheet's own way, where it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Numbers {
    /// How many decimals every number has (`.750`); none for no more
    /// than it needs, at most four.
    #[serde(default)]
    pub decimals: Option<u8>,
    /// Whether a number less than one is written without its zero:
    /// `.75`, as inch drawings have it.
    #[serde(default)]
    pub bare: bool,
    /// Whether the decimals follow a point, not a comma.
    #[serde(default)]
    pub point: bool,
    /// Whether, with `decimals`, a number has no more of them than it
    /// needs — `.5`, `1.125`, `2.75` — `decimals` the most it has.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trim: bool,
    /// Whether a tolerance class is written with its deviations after it
    /// in brackets, `⌀20 H7 (+0,021/0)`; else as the class alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fit_deviations: bool,
}

thread_local! {
    static NUMBERS: std::cell::Cell<Numbers> = const {
        std::cell::Cell::new(Numbers {
            decimals: None,
            bare: false,
            point: false,
            trim: false,
            fit_deviations: false,
        })
    };
}

/// What `work` gives with every [`number`] it writes written as
/// `numbers` say.
pub fn written<T>(numbers: Numbers, work: impl FnOnce() -> T) -> T {
    let before = NUMBERS.replace(numbers);
    let made = work();
    NUMBERS.set(before);
    made
}

/// How numbers are written just now.
pub fn numbers() -> Numbers {
    NUMBERS.get()
}

/// A number as a drawing writes it (ISO 129-1): with a comma, no more
/// decimals than it has, at most four — or as the sheet it is written
/// on says (see [`written`]).
pub fn number(x: f64) -> String {
    let how = NUMBERS.get();
    let text = match how.decimals {
        Some(decimals) if !how.trim => format!("{x:.*}", decimals as usize),
        Some(decimals) => {
            let text = format!("{x:.*}", decimals as usize);
            match text.contains('.') {
                true => text.trim_end_matches('0').trim_end_matches('.').to_owned(),
                false => text,
            }
        }
        None => {
            let text = format!("{x:.4}");
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        }
    };
    let zero = |t: &str| {
        t.trim_start_matches('-')
            .chars()
            .all(|c| c == '0' || c == '.')
    };
    let text = if zero(&text) && text.starts_with('-') {
        text[1..].to_owned()
    } else {
        text
    };
    let text = match (how.bare, text.strip_prefix("0."), text.strip_prefix("-0.")) {
        (true, Some(rest), _) => format!(".{rest}"),
        (true, _, Some(rest)) => format!("-.{rest}"),
        _ => text,
    };
    let text = if how.point {
        text
    } else {
        text.replace('.', ",")
    };
    text.replace('-', "−")
}

/// How many decimals a number has as written: after its comma (or
/// point).
fn decimals_of(text: &str) -> usize {
    text.rsplit_once([',', '.']).map_or(0, |(_, d)| d.len())
}

/// `text` with zeros after it to `decimals` decimals.
fn padded(text: String, decimals: usize) -> String {
    let had = decimals_of(&text);
    if had >= decimals {
        return text;
    }
    let comma = if numbers().point { "." } else { "," };
    let text = if had == 0 { text + comma } else { text };
    text + &"0".repeat(decimals - had)
}

/// Two numbers written with as many decimals as the one that has more
/// (ISO 129-1): `10,2` and `9,9`, `20,021` and `20,000`.
pub fn alike(a: f64, b: f64) -> [String; 2] {
    let (a, b) = (number(a), number(b));
    let d = decimals_of(&a).max(decimals_of(&b));
    [padded(a, d), padded(b, d)]
}

/// An upper and a lower deviation as a drawing writes them (ISO 129-1):
/// each with its sign and as many decimals as the other, but a deviation
/// of nothing, which is a bare `0`; `unit` after each but that (`°`).
pub fn deviations(upper: f64, lower: f64, unit: &str) -> [String; 2] {
    let zero = |x: f64| number(x.abs()) == "0";
    let [u, l] = alike(upper.abs(), lower.abs());
    let d = match (zero(upper), zero(lower)) {
        (true, false) => decimals_of(&l),
        (false, true) => decimals_of(&u),
        _ => decimals_of(&u).max(decimals_of(&l)),
    };
    let signed = |x: f64, text: String| match zero(x) {
        true => "0".to_owned(),
        false => format!(
            "{}{}{unit}",
            if x > 0.0 { "+" } else { "−" },
            padded(text, d)
        ),
    };
    [signed(upper, u), signed(lower, l)]
}

impl Tolerance {
    /// What follows the value `size` (its `unit` after each number of
    /// it: `°` of an angle): ` ±0,1`, ` +0,2/−0,1`, ` H7` — with the
    /// sheet's [`Numbers::fit_deviations`] ` H7 (+0,021/0)`; nothing
    /// where the general tolerance applies.
    pub fn suffix(&self, size: f64, unit: &str) -> String {
        match self {
            Tolerance::General | Tolerance::Reference => String::new(),
            Tolerance::Symmetric(t) => format!(" ±{}{unit}", number(t.abs())),
            Tolerance::Deviations { upper, lower } => {
                let [u, l] = deviations(*upper, *lower, unit);
                format!(" {u}/{l}")
            }
            Tolerance::Limits { upper, lower } => {
                let [u, l] = alike(size + upper, size + lower);
                format!(" ({l}{unit}…{u}{unit})")
            }
            Tolerance::Fit(class) => match self.fit_deviations(size) {
                Some([u, l]) => format!(" {class} ({u}/{l})"),
                None => format!(" {class}"),
            },
        }
    }

    /// Of a tolerance class written with its deviations (as the sheet
    /// says, see [`Numbers::fit_deviations`]): them, upper and lower.
    pub fn fit_deviations(&self, size: f64) -> Option<[String; 2]> {
        let Tolerance::Fit(class) = self else {
            return None;
        };
        if !numbers().fit_deviations {
            return None;
        }
        let (upper, lower) = super::fits::limits(class, size).ok()?;
        Some(deviations(upper, lower, ""))
    }

    /// Whether it can be held to of the size `size`: a tolerance class
    /// that ISO 286 tables for it (else why not), deviations the upper
    /// over the lower.
    pub fn check(&self, size: f64) -> Result<(), String> {
        match self {
            Tolerance::Fit(class) => super::fits::limits(class, size).map(|_| ()),
            Tolerance::Deviations { upper, lower } | Tolerance::Limits { upper, lower }
                if upper < lower =>
            {
                Err(format!(
                    "the upper deviation {upper} is below the lower one {lower}"
                ))
            }
            Tolerance::Symmetric(t) if *t <= 0.0 => {
                Err(format!("a ± tolerance is more than nothing, not {t}"))
            }
            _ => Ok(()),
        }
    }

    /// The same numbers written the way of `kind`: deviations stay
    /// deviations when they are to be written as limits, and a
    /// symmetric tolerance becomes as much up as down.
    #[must_use]
    pub fn as_kind(&self, kind: &Tolerance) -> Tolerance {
        let (upper, lower) = match self {
            Tolerance::Symmetric(t) => (t.abs(), -t.abs()),
            Tolerance::Deviations { upper, lower } | Tolerance::Limits { upper, lower } => {
                (*upper, *lower)
            }
            Tolerance::General | Tolerance::Fit(_) | Tolerance::Reference => return kind.clone(),
        };
        match kind {
            Tolerance::Symmetric(_) => Tolerance::Symmetric(upper.abs().max(lower.abs())),
            Tolerance::Deviations { .. } => Tolerance::Deviations { upper, lower },
            Tolerance::Limits { .. } => Tolerance::Limits { upper, lower },
            other => other.clone(),
        }
    }
}

/// A direction across `axis`; any, for a ball (which has none).
fn across(axis: Point3) -> Point3 {
    match axis.try_normalize() {
        Some(axis) => axis.any_orthonormal_vector(),
        None => Point3::X,
    }
}

impl Measured {
    /// Where its text stands by itself: off what it measures, by a
    /// share of how large that is (`reach`, for an angle, which has no
    /// size of its own).
    pub fn beside(&self, reach: f64) -> Point3 {
        match self {
            Measured::Length { from, to, .. } => {
                let across = (*to - *from).any_orthonormal_vector();
                (*from + *to) / 2.0 + across * (from.distance(*to) * 0.3 + 1e-9)
            }
            Measured::Diameter {
                centre,
                axis,
                radius,
            }
            | Measured::Radius {
                centre,
                axis,
                radius,
            } => *centre + across(*axis) * (radius * 1.6),
            Measured::Radial {
                centre,
                axis,
                inner,
                outer,
            } => *centre + across(*axis) * ((inner + outer) / 2.0),
            Measured::Angle { at, first, second } => {
                let between = (*first + *second).normalize_or_zero();
                *at + between * reach
            }
        }
    }
}

impl Annotation {
    /// A size a step holds the model to: `measure` of its arguments,
    /// `value` as it is in the part built, where `measured` says. It
    /// shows on the model's drawing, and on the model where what is
    /// said of it is looked at.
    pub fn held(measured: Measured, value: f64, measure: &crate::Measure, reach: f64) -> Self {
        Self {
            sketch: None,
            at: measured.beside(reach),
            note: Note::Dimension {
                measured,
                value,
                tolerance: measure.tolerance.clone(),
                elsewhere: Vec::new(),
            },
        }
    }

    /// What it says, in a line: `⌀10 H7`, `25 ±0,1`, `Flatness 0,05`,
    /// `Position ⌀0,1 | A | B`, `Datum A`.
    pub fn text(&self) -> String {
        match &self.note {
            Note::Dimension {
                measured,
                value,
                tolerance,
                ..
            } => {
                let prefix = match measured {
                    Measured::Diameter { .. } => "⌀",
                    Measured::Radius { .. } => "R",
                    _ => "",
                };
                let unit = match measured {
                    Measured::Angle { .. } => "°",
                    _ => "",
                };
                let said = format!(
                    "{prefix}{}{unit}{}",
                    number(*value),
                    tolerance.suffix(*value, unit)
                );
                match tolerance {
                    Tolerance::Reference => format!("({said})"),
                    _ => said,
                }
            }
            Note::Hole { lines, .. } => lines
                .iter()
                .map(|line| line.iter().map(Callout::said).collect::<String>())
                .collect::<Vec<_>>()
                .join(" "),
            Note::Datum { letter, .. } => format!("Datum {letter}"),
            Note::Geometric {
                characteristic,
                value,
                diametral,
                datums,
                ..
            } => {
                let zone = if *diametral { "⌀" } else { "" };
                let mut text = format!("{} {zone}{}", characteristic.label(), number(*value));
                for datum in datums {
                    text += &format!(" | {datum}");
                }
                text
            }
        }
    }

    /// The points of the model it is about: where its leaders go.
    pub fn anchors(&self) -> Vec<Point3> {
        match &self.note {
            Note::Dimension { measured, .. } => match measured {
                Measured::Length { from, to, .. } => vec![*from, *to],
                Measured::Diameter { centre, .. }
                | Measured::Radius { centre, .. }
                | Measured::Radial { centre, .. } => {
                    vec![*centre]
                }
                Measured::Angle { at, .. } => vec![*at],
            },
            Note::Datum { on, .. } | Note::Geometric { on, .. } => vec![*on],
            Note::Hole { centre, .. } => vec![*centre],
        }
    }
}

#[cfg(test)]
mod number_tests {
    use super::*;

    #[test]
    fn numbers_are_written_as_a_sheet_says() {
        // By themselves: a comma, no more decimals than needed.
        assert_eq!(number(0.75), "0,75");
        assert_eq!(number(15.5892), "15,5892");
        assert_eq!(number(-0.00001), "0");
        // An inch sheet: three decimals after a point, no zero before it.
        let inch = Numbers {
            decimals: Some(3),
            bare: true,
            point: true,
            trim: false,
            fit_deviations: false,
        };
        let said = written(inch, || {
            [number(0.75), number(4.5), number(1.0), number(-0.5)]
        });
        assert_eq!(said, [".750", "4.500", "1.000", "−.500"]);
        // Two decimals round a number that has more; and afterwards
        // numbers are written as before.
        let two = Numbers {
            decimals: Some(2),
            ..Numbers::default()
        };
        assert_eq!(written(two, || number(15.5892)), "15,59");
        assert_eq!(number(15.5892), "15,5892");
        // Trimmed: no more decimals than needed, three at the most.
        let trimmed = Numbers { trim: true, ..inch };
        let said = written(trimmed, || {
            [
                number(0.5),
                number(1.125),
                number(2.75),
                number(3.0),
                number(0.12345),
            ]
        });
        assert_eq!(said, [".5", "1.125", "2.75", "3", ".123"]);
    }

    #[test]
    fn tolerances_are_written_as_iso_129_has_them() {
        // Deviations with as many decimals as each other; a zero bare.
        assert_eq!(deviations(0.25, -0.1, ""), ["+0,25", "−0,10"]);
        assert_eq!(deviations(0.021, 0.0, ""), ["+0,021", "0"]);
        assert_eq!(deviations(0.5, -0.5, "°"), ["+0,5°", "−0,5°"]);
        // Limits alike; an angle's ± with its degree sign.
        assert_eq!(alike(20.021, 20.0), ["20,021", "20,000"]);
        assert_eq!(Tolerance::Symmetric(0.5).suffix(30.0, "°"), " ±0,5°");
        // A class alone, or with its deviations where the sheet says.
        let fit = Tolerance::Fit("H7".into());
        assert_eq!(fit.suffix(20.0, ""), " H7");
        let with = Numbers {
            fit_deviations: true,
            ..Numbers::default()
        };
        assert_eq!(written(with, || fit.suffix(20.0, "")), " H7 (+0,021/0)");
        assert!(Tolerance::Fit("H77".into()).check(20.0).is_err());
        assert!(Tolerance::Fit("k6".into()).check(20.0).is_ok());
    }
}
