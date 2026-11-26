//! The limits of ISO 286 tolerance classes: of holes (upper case, `H7`,
//! `N9`) and of shafts (lower case, `k6`, `f7`), for nominal sizes up
//! to 500 mm.

/// The upper ends of the ranges of nominal sizes (ISO 286-1): a size
/// is in the first range whose end it does not pass.
const RANGES: [f64; 13] = [
    3.0, 6.0, 10.0, 18.0, 30.0, 50.0, 80.0, 120.0, 180.0, 250.0, 315.0, 400.0, 500.0,
];

/// The standard tolerance grades IT4 to IT14 by range, in micrometres
/// (ISO 286-1, table 1).
const GRADES: [(u32, [f64; 13]); 11] = [
    (
        4,
        [3., 4., 4., 5., 6., 7., 8., 10., 12., 14., 16., 18., 20.],
    ),
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

/// The fundamental deviations of shafts d to g: how far below the
/// nominal size their upper limit is, by range, in micrometres (ISO
/// 286-1, table 2) — and of holes D to G how far above it their lower
/// limit is.
const CLEARANCE: [(char, [f64; 13]); 4] = [
    (
        'd',
        [
            20., 30., 40., 50., 65., 80., 100., 120., 145., 170., 190., 210., 230.,
        ],
    ),
    (
        'e',
        [
            14., 20., 25., 32., 40., 50., 60., 72., 85., 100., 110., 125., 135.,
        ],
    ),
    (
        'f',
        [
            6., 10., 13., 16., 20., 25., 30., 36., 43., 50., 56., 62., 68.,
        ],
    ),
    (
        'g',
        [2., 4., 5., 6., 7., 9., 10., 12., 14., 15., 17., 18., 20.],
    ),
];

/// The fundamental deviations of shafts k (grades 4 to 7), m, n and p:
/// how far above the nominal size their lower limit is, by range, in
/// micrometres (ISO 286-1, table 3).
const INTERFERENCE: [(char, [f64; 13]); 4] = [
    ('k', [0., 1., 1., 1., 2., 2., 2., 3., 3., 4., 4., 4., 5.]),
    (
        'm',
        [2., 4., 6., 7., 8., 9., 11., 13., 15., 17., 20., 21., 23.],
    ),
    (
        'n',
        [
            4., 8., 10., 12., 15., 17., 20., 23., 27., 31., 34., 37., 40.,
        ],
    ),
    (
        'p',
        [
            6., 12., 15., 18., 22., 26., 32., 37., 43., 50., 56., 62., 68.,
        ],
    ),
];

/// The tolerance classes a table has, holes first: what a dialog
/// offers and an error names.
pub const CLASSES: &str = "holes D E F G H JS K M N P, shafts d e f g h js k m n p; grades 5 to 14 \
                           (4 for shafts and H); sizes to 500 mm";

/// The upper and the lower deviation, in millimetres, of the nominal
/// size `size` in the ISO 286 tolerance class `class`: a hole's in upper
/// case (`H7`, `N9`, `JS8`), a shaft's in lower case (`k6`, `f7`,
/// `js6`). An error says why there is none: a class or a grade the
/// tables do not have, a size out of them.
pub fn limits(class: &str, size: f64) -> Result<(f64, f64), String> {
    let letters = class.trim_end_matches(|c: char| c.is_ascii_digit());
    let unknown = || format!("there is no ISO 286 class {class:?} ({CLASSES})");
    let grade: u32 = class[letters.len()..].parse().map_err(|_| unknown())?;
    let hole = match letters {
        "D" | "E" | "F" | "G" | "H" | "JS" | "K" | "M" | "N" | "P" => true,
        "d" | "e" | "f" | "g" | "h" | "js" | "k" | "m" | "n" | "p" => false,
        _ => return Err(unknown()),
    };
    let range = RANGES
        .iter()
        .position(|r| size <= *r + 1e-9)
        .filter(|_| size > 0.0)
        .ok_or_else(|| format!("{class} is tabled for sizes above 0 up to 500 mm, not {size}"))?;
    let it = |grade: u32| GRADES.iter().find(|g| g.0 == grade).map(|g| g.1[range]);
    let width = it(grade).ok_or_else(unknown)?;
    let lowest = letters.to_ascii_lowercase();
    let table = |of: &[(char, [f64; 13])]| {
        of.iter()
            .find(|row| lowest.starts_with(row.0))
            .map(|row| row.1[range])
    };
    // A shaft's limits, in micrometres: from its fundamental deviation.
    let shaft = |grade: u32, width: f64| -> (f64, f64) {
        match lowest.as_str() {
            "h" => (0.0, -width),
            "js" => (width / 2.0, -width / 2.0),
            "k" => {
                // (Only grades 4 to 7 are lifted, and none to 3 mm.)
                let ei = match (4..=7).contains(&grade) {
                    true => table(&INTERFERENCE).unwrap_or(0.0),
                    false => 0.0,
                };
                (ei + width, ei)
            }
            "m" | "n" | "p" => {
                let ei = table(&INTERFERENCE).unwrap_or(0.0);
                (ei + width, ei)
            }
            _ => {
                let es = -table(&CLEARANCE).unwrap_or(0.0);
                (es, es - width)
            }
        }
    };
    let (upper, lower) = if !hole {
        shaft(grade, width)
    } else {
        // Holes K to P: their upper limit is the shaft's lower one
        // turned over, raised by Δ — the grade's width less the one
        // below — for the finer grades (ISO 286-1, table 3).
        let delta = || -> Result<f64, String> {
            if range == 0 {
                return Ok(0.0);
            }
            Ok(width - it(grade - 1).ok_or_else(unknown)?)
        };
        let ei = || table(&INTERFERENCE).unwrap_or(0.0);
        let upper = match letters {
            "H" => width,
            "JS" => width / 2.0,
            "K" if grade <= 8 && range > 0 => -ei() + delta()?,
            "K" => 0.0,
            "M" if range == 0 => -2.0,
            "M" if grade <= 8 => -ei() + delta()?,
            "M" => -ei(),
            "N" if range == 0 => -4.0,
            "N" if grade <= 8 => -ei() + delta()?,
            "N" => 0.0,
            "P" if grade <= 7 => -ei() + delta()?,
            "P" => -ei(),
            _ => table(&CLEARANCE).unwrap_or(0.0) + width,
        };
        if grade < 5 && letters != "H" {
            return Err(unknown());
        }
        (upper, upper - width)
    };
    Ok((upper / 1000.0, lower / 1000.0))
}

/// Whether `class` is said like an ISO 286 tolerance class: a hole's
/// letters in upper case or a shaft's in lower case, then its grade.
pub fn is_class(class: &str) -> bool {
    let letters = class.trim_end_matches(|c: char| c.is_ascii_digit());
    let grade = class.len() - letters.len();
    (1..=2).contains(&letters.len())
        && (1..=2).contains(&grade)
        && letters.chars().all(|c| c.is_ascii_alphabetic())
        && (letters == letters.to_ascii_uppercase() || letters == letters.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn um(class: &str, size: f64) -> (i64, i64) {
        let (u, l) = limits(class, size).expect(class);
        ((u * 1000.0).round() as i64, (l * 1000.0).round() as i64)
    }

    #[test]
    fn classes_have_the_limits_of_iso_286() {
        // The brief's three, and the usual neighbours.
        assert_eq!(um("H7", 20.0), (21, 0));
        assert_eq!(um("k6", 20.0), (15, 2));
        assert_eq!(um("N9", 6.0), (0, -30));
        assert_eq!(um("H7", 10.0), (15, 0));
        assert_eq!(um("F8", 20.0), (53, 20));
        assert_eq!(um("f7", 8.0), (-13, -28));
        assert_eq!(um("g6", 20.0), (-7, -20));
        assert_eq!(um("h6", 20.0), (0, -13));
        assert_eq!(limits("js6", 20.0), Ok((0.0065, -0.0065)));
        assert_eq!(um("m6", 20.0), (21, 8));
        assert_eq!(um("n6", 20.0), (28, 15));
        assert_eq!(um("p6", 20.0), (35, 22));
        assert_eq!(um("K7", 20.0), (6, -15));
        assert_eq!(um("M7", 20.0), (0, -21));
        assert_eq!(um("N7", 20.0), (-7, -28));
        assert_eq!(um("P7", 20.0), (-14, -35));
        assert_eq!(um("P9", 6.0), (-12, -42));
        assert_eq!(um("K6", 52.0), (4, -15));
        assert_eq!(um("H7", 52.0), (30, 0));
        assert_eq!(um("k6", 3.0), (6, 0));
        // Not in the tables: an error, not a guess.
        assert!(limits("Q3", 10.0).is_err());
        assert!(limits("h7", 600.0).is_err());
        assert!(limits("Hk7", 10.0).is_err());
        assert!(limits("Js7", 10.0).is_err());
        assert!(is_class("H7") && is_class("js6") && !is_class("Js6") && !is_class("H"));
    }
}
