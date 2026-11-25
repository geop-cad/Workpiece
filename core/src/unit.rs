//! The unit of length a part's numbers are in.

/// A unit of length: what a part's numbers count.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Unit {
    /// Millimetres.
    #[default]
    Millimetre,
    /// Centimetres.
    Centimetre,
    /// Metres.
    Metre,
    /// Inches.
    Inch,
}

impl Unit {
    /// Every unit.
    pub const ALL: [Unit; 4] = [Unit::Millimetre, Unit::Centimetre, Unit::Metre, Unit::Inch];

    /// Its short name: `mm`, `in`.
    pub fn label(self) -> &'static str {
        match self {
            Unit::Millimetre => "mm",
            Unit::Centimetre => "cm",
            Unit::Metre => "m",
            Unit::Inch => "in",
        }
    }

    /// How many millimetres one of it is.
    pub fn millimetres(self) -> f64 {
        match self {
            Unit::Millimetre => 1.0,
            Unit::Centimetre => 10.0,
            Unit::Metre => 1000.0,
            Unit::Inch => 25.4,
        }
    }

    /// Whether nothing needs saying about it: it is the default.
    pub fn is_default(&self) -> bool {
        *self == Unit::default()
    }
}
