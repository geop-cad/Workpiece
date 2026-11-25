//! What the standards say of threads and of the holes for screws, in
//! millimetres: ISO metric threads (ISO 261, ISO 262), the holes screws
//! go through (ISO 273), counterbores for their heads (DIN 974),
//! countersinks (DIN 74, ISO 15065), and the limits of tolerance
//! classes (ISO 286).

/// A thread size, and what goes with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// Its name: `M6`.
    pub name: &'static str,
    /// The thread's (major) diameter.
    pub diameter: f64,
    /// Its coarse pitch (ISO 261).
    pub coarse: f64,
    /// Its fine pitches, the coarsest first (ISO 262).
    pub fine: &'static [f64],
    /// The hole a screw of it goes through (ISO 273): fine, medium and
    /// coarse.
    pub clearance: [f64; 3],
    /// The counterbore for a hexagon socket head cap screw (ISO 4762):
    /// its diameter (DIN 974-1, series 1).
    pub cap: Option<f64>,
    /// The counterbore for a hexagon head screw (ISO 4014, ISO 4017),
    /// wide enough for a socket wrench: its diameter (DIN 974-2,
    /// series 1) and the head's height.
    pub hexagon: Option<(f64, f64)>,
    /// The countersink for a hexagon socket countersunk screw
    /// (ISO 10642): its diameter at the face (DIN 74, form F).
    pub socket_sink: Option<f64>,
    /// The countersink for a slotted or cross recessed countersunk
    /// screw (ISO 2009, ISO 7046): its diameter at the face
    /// (ISO 15065).
    pub slotted_sink: Option<f64>,
}

const fn size(
    name: &'static str,
    diameter: f64,
    coarse: f64,
    fine: &'static [f64],
    clearance: [f64; 3],
) -> Size {
    Size {
        name,
        diameter,
        coarse,
        fine,
        clearance,
        cap: None,
        hexagon: None,
        socket_sink: None,
        slotted_sink: None,
    }
}

impl Size {
    const fn cap(mut self, bore: f64) -> Self {
        self.cap = Some(bore);
        self
    }

    const fn hexagon(mut self, bore: f64, head: f64) -> Self {
        self.hexagon = Some((bore, head));
        self
    }

    const fn socket_sink(mut self, at_face: f64) -> Self {
        self.socket_sink = Some(at_face);
        self
    }

    const fn slotted_sink(mut self, at_face: f64) -> Self {
        self.slotted_sink = Some(at_face);
        self
    }

    /// Its pitches: the coarse one, then the fine ones.
    pub fn pitches(&self) -> impl Iterator<Item = f64> + '_ {
        std::iter::once(self.coarse).chain(self.fine.iter().copied())
    }

    /// What its thread is called with the pitch `pitch`: `M6`, and with
    /// a fine pitch `M8×1`.
    pub fn designation(&self, pitch: f64) -> String {
        if (pitch - self.coarse).abs() < 1e-9 {
            self.name.to_owned()
        } else {
            format!("{}×{pitch}", self.name)
        }
    }
}

/// The sizes of ISO metric threads from M1 to M64 (ISO 261, first to
/// third choice, as far as ISO 273 has holes for them).
pub const SIZES: [Size; 35] = [
    size("M1", 1.0, 0.25, &[0.2], [1.1, 1.2, 1.3]),
    size("M1.2", 1.2, 0.25, &[0.2], [1.3, 1.4, 1.5]),
    size("M1.4", 1.4, 0.3, &[0.2], [1.5, 1.6, 1.8]),
    size("M1.6", 1.6, 0.35, &[0.2], [1.7, 1.8, 2.0])
        .cap(3.5)
        .slotted_sink(3.6),
    size("M1.8", 1.8, 0.35, &[0.2], [2.0, 2.1, 2.2]),
    size("M2", 2.0, 0.4, &[0.25], [2.2, 2.4, 2.6])
        .cap(4.4)
        .slotted_sink(4.4),
    size("M2.5", 2.5, 0.45, &[0.35], [2.7, 2.9, 3.1])
        .cap(5.5)
        .slotted_sink(5.5),
    size("M3", 3.0, 0.5, &[0.35], [3.2, 3.4, 3.6])
        .cap(6.5)
        .hexagon(11.0, 2.0)
        .socket_sink(6.94)
        .slotted_sink(6.3),
    size("M3.5", 3.5, 0.6, &[0.35], [3.7, 3.9, 4.2]).slotted_sink(8.2),
    size("M4", 4.0, 0.7, &[0.5], [4.3, 4.5, 4.8])
        .cap(8.0)
        .hexagon(13.0, 2.8)
        .socket_sink(9.18)
        .slotted_sink(9.4),
    size("M4.5", 4.5, 0.75, &[0.5], [4.8, 5.0, 5.3]),
    size("M5", 5.0, 0.8, &[0.5], [5.3, 5.5, 5.8])
        .cap(10.0)
        .hexagon(15.0, 3.5)
        .socket_sink(11.47)
        .slotted_sink(10.4),
    size("M6", 6.0, 1.0, &[0.75], [6.4, 6.6, 7.0])
        .cap(11.0)
        .hexagon(18.0, 4.0)
        .socket_sink(13.71)
        .slotted_sink(12.6),
    size("M7", 7.0, 1.0, &[0.75], [7.4, 7.6, 8.0]),
    size("M8", 8.0, 1.25, &[1.0, 0.75], [8.4, 9.0, 10.0])
        .cap(15.0)
        .hexagon(24.0, 5.3)
        .socket_sink(18.25)
        .slotted_sink(17.3),
    size("M10", 10.0, 1.5, &[1.25, 1.0, 0.75], [10.5, 11.0, 12.0])
        .cap(18.0)
        .hexagon(28.0, 6.4)
        .socket_sink(22.73)
        .slotted_sink(20.0),
    size("M12", 12.0, 1.75, &[1.5, 1.25, 1.0], [13.0, 13.5, 14.5])
        .cap(20.0)
        .hexagon(33.0, 7.5)
        .socket_sink(27.21),
    size("M14", 14.0, 2.0, &[1.5, 1.25, 1.0], [15.0, 15.5, 16.5])
        .cap(24.0)
        .hexagon(36.0, 8.8)
        .socket_sink(31.19),
    size("M16", 16.0, 2.0, &[1.5, 1.0], [17.0, 17.5, 18.5])
        .cap(26.0)
        .hexagon(40.0, 10.0)
        .socket_sink(33.99),
    size("M18", 18.0, 2.5, &[2.0, 1.5, 1.0], [19.0, 20.0, 21.0])
        .cap(30.0)
        .hexagon(43.0, 11.5),
    size("M20", 20.0, 2.5, &[2.0, 1.5, 1.0], [21.0, 22.0, 24.0])
        .cap(33.0)
        .hexagon(46.0, 12.5)
        .socket_sink(40.71),
    size("M22", 22.0, 2.5, &[2.0, 1.5, 1.0], [23.0, 24.0, 26.0])
        .cap(36.0)
        .hexagon(54.0, 14.0),
    size("M24", 24.0, 3.0, &[2.0, 1.5, 1.0], [25.0, 26.0, 28.0])
        .cap(40.0)
        .hexagon(58.0, 15.0),
    size("M27", 27.0, 3.0, &[2.0, 1.5, 1.0], [28.0, 30.0, 32.0])
        .cap(46.0)
        .hexagon(61.0, 17.0),
    size("M30", 30.0, 3.5, &[3.0, 2.0, 1.5, 1.0], [31.0, 33.0, 35.0])
        .cap(50.0)
        .hexagon(73.0, 18.7),
    size("M33", 33.0, 3.5, &[3.0, 2.0, 1.5], [34.0, 36.0, 38.0])
        .cap(54.0)
        .hexagon(76.0, 21.0),
    size("M36", 36.0, 4.0, &[3.0, 2.0, 1.5], [37.0, 39.0, 42.0])
        .cap(58.0)
        .hexagon(82.0, 22.5),
    size("M39", 39.0, 4.0, &[3.0, 2.0, 1.5], [40.0, 42.0, 45.0]),
    size("M42", 42.0, 4.5, &[4.0, 3.0, 2.0, 1.5], [43.0, 45.0, 48.0])
        .cap(69.0)
        .hexagon(98.0, 26.0),
    size("M45", 45.0, 4.5, &[4.0, 3.0, 2.0, 1.5], [46.0, 48.0, 52.0]),
    size("M48", 48.0, 5.0, &[4.0, 3.0, 2.0, 1.5], [50.0, 52.0, 56.0])
        .cap(78.0)
        .hexagon(112.0, 30.0),
    size("M52", 52.0, 5.0, &[4.0, 3.0, 2.0, 1.5], [54.0, 56.0, 62.0]),
    size("M56", 56.0, 5.5, &[4.0, 3.0, 2.0, 1.5], [58.0, 62.0, 66.0]),
    size("M60", 60.0, 5.5, &[4.0, 3.0, 2.0, 1.5], [62.0, 66.0, 70.0]),
    size("M64", 64.0, 6.0, &[4.0, 3.0, 2.0, 1.5], [66.0, 70.0, 74.0]),
];

/// The names of the sizes, for a dialog.
pub fn size_names() -> Vec<String> {
    SIZES.iter().map(|s| s.name.to_owned()).collect()
}

/// The size called `name`.
pub fn sized(name: &str) -> Option<&'static Size> {
    SIZES.iter().find(|s| s.name == name)
}

/// The size whose thread has the diameter `diameter`, or is the next
/// below it: what a rod or a hole of that width is threaded as.
pub fn size_for(diameter: f64) -> Option<&'static Size> {
    SIZES.iter().rev().find(|s| s.diameter <= diameter + 1e-6)
}

/// The fine pitches of the size called `size`, as a dialog shows them.
pub fn fine_names(size: Option<&str>) -> Vec<String> {
    size.and_then(sized)
        .map(|s| s.fine.iter().map(|p| format!("{p}")).collect())
        .unwrap_or_default()
}

/// The diameter of the drill for a thread of `diameter` and `pitch`
/// that is cut with a tap (ISO 2306): the diameter less the pitch, to
/// the drill there is.
pub fn tap_drill(diameter: f64, pitch: f64) -> f64 {
    const ROUNDED: [(f64, f64, f64); 5] = [
        (4.5, 0.75, 3.7),
        (10.0, 1.25, 8.8),
        (12.0, 1.75, 10.2),
        (12.0, 1.25, 10.8),
        (14.0, 1.25, 12.8),
    ];
    ROUNDED
        .iter()
        .find(|r| (r.0 - diameter).abs() < 1e-9 && (r.1 - pitch).abs() < 1e-9)
        .map_or(diameter - pitch, |r| r.2)
}

/// How deep a thread of `pitch` is cut: the height of the ISO profile
/// (ISO 68-1), flank to flank over the root's rounding.
pub fn thread_depth(pitch: f64) -> f64 {
    0.6134 * pitch
}

/// How much deeper than a head is high its counterbore is (DIN 974),
/// for a thread of `diameter`.
pub fn head_allowance(diameter: f64) -> f64 {
    if diameter <= 6.0 {
        0.4
    } else if diameter <= 20.0 {
        0.6
    } else if diameter <= 27.0 {
        0.8
    } else {
        1.0
    }
}

/// The upper ends of the ranges of nominal sizes (ISO 286-1).
const RANGES: [f64; 13] = [
    3.0, 6.0, 10.0, 18.0, 30.0, 50.0, 80.0, 120.0, 180.0, 250.0, 315.0, 400.0, 500.0,
];

/// The standard tolerance grades IT5 to IT14 by range, in micrometres.
const GRADES: [(u32, [f64; 13]); 10] = [
    (
        5,
        [4., 5., 6., 8., 9., 11., 13., 15., 18., 20., 23., 25., 27.],
    ),
    (
        6,
        [6., 8., 9., 11., 13., 16., 19., 22., 25., 29., 32., 36., 40.],
    ),
    (
        7,
        [
            10., 12., 15., 18., 21., 25., 30., 35., 40., 46., 52., 57., 63.,
        ],
    ),
    (
        8,
        [
            14., 18., 22., 27., 33., 39., 46., 54., 63., 72., 81., 89., 97.,
        ],
    ),
    (
        9,
        [
            25., 30., 36., 43., 52., 62., 74., 87., 100., 115., 130., 140., 155.,
        ],
    ),
    (
        10,
        [
            40., 48., 58., 70., 84., 100., 120., 140., 160., 185., 210., 230., 250.,
        ],
    ),
    (
        11,
        [
            60., 75., 90., 110., 130., 160., 190., 220., 250., 290., 320., 360., 400.,
        ],
    ),
    (
        12,
        [
            100., 120., 150., 180., 210., 250., 300., 350., 400., 460., 520., 570., 630.,
        ],
    ),
    (
        13,
        [
            140., 180., 220., 270., 330., 390., 460., 540., 630., 720., 810., 890., 970.,
        ],
    ),
    (
        14,
        [
            250., 300., 360., 430., 520., 620., 740., 870., 1000., 1150., 1300., 1400., 1550.,
        ],
    ),
];

/// The lower deviations of holes D to G by range, in micrometres.
const HOLES: [(&str, [f64; 13]); 4] = [
    (
        "D",
        [
            20., 30., 40., 50., 65., 80., 100., 120., 145., 170., 190., 210., 230.,
        ],
    ),
    (
        "E",
        [
            14., 20., 25., 32., 40., 50., 60., 72., 85., 100., 110., 125., 135.,
        ],
    ),
    (
        "F",
        [
            6., 10., 13., 16., 20., 25., 30., 36., 43., 50., 56., 62., 68.,
        ],
    ),
    (
        "G",
        [2., 4., 5., 6., 7., 9., 10., 12., 14., 15., 17., 18., 20.],
    ),
];

/// The tolerance classes a bore is offered in a dialog.
pub const CLASSES: [&str; 13] = [
    "H6", "H7", "H8", "H9", "H10", "H11", "H12", "H13", "G7", "F7", "F8", "E9", "D10",
];

/// The names of [`CLASSES`], for a dialog.
pub fn class_names() -> Vec<String> {
    CLASSES.iter().map(|c| (*c).to_owned()).collect()
}

/// The upper and the lower deviation of a hole of the nominal size
/// `diameter` in the tolerance class `class` (ISO 286): `H7`, `F8`,
/// `JS9`; holes D to H and JS, grades 5 to 14, sizes to 500.
pub fn hole_limits(class: &str, diameter: f64) -> Option<(f64, f64)> {
    let letters = class.trim_end_matches(|c: char| c.is_ascii_digit());
    let grade: u32 = class[letters.len()..].parse().ok()?;
    let range = RANGES.iter().position(|r| diameter <= *r + 1e-9)?;
    if diameter <= 0.0 {
        return None;
    }
    let width = GRADES.iter().find(|g| g.0 == grade)?.1[range] / 1000.0;
    match letters {
        "H" => Some((width, 0.0)),
        "JS" => Some((width / 2.0, -width / 2.0)),
        _ => {
            let lower = HOLES.iter().find(|h| h.0 == letters)?.1[range] / 1000.0;
            Some((lower + width, lower))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_as_the_standards_have_them() {
        let m6 = sized("M6").expect("M6");
        assert_eq!((m6.coarse, m6.clearance[1], m6.cap), (1.0, 6.6, Some(11.0)));
        assert_eq!(tap_drill(6.0, 1.0), 5.0);
        assert_eq!(tap_drill(12.0, 1.75), 10.2);
        assert_eq!(sized("M10").expect("M10").designation(1.25), "M10×1.25");
        assert_eq!(size_for(6.2).map(|s| s.name), Some("M6"));
        // Every size is wider than the one before, and its holes too.
        for pair in SIZES.windows(2) {
            assert!(pair[0].diameter < pair[1].diameter);
            assert!(pair[0].clearance[1] < pair[1].clearance[1]);
            assert!(pair[1].clearance[0] > pair[1].diameter);
        }
        // ⌀10 H7 is +0.015 / 0; ⌀20 F8 is +0.053 / +0.020.
        assert_eq!(hole_limits("H7", 10.0), Some((0.015, 0.0)));
        let (upper, lower) = hole_limits("F8", 20.0).expect("F8");
        assert!((upper - 0.053).abs() < 1e-12 && (lower - 0.020).abs() < 1e-12);
        assert_eq!(hole_limits("Q3", 10.0), None);
    }
}
