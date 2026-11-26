//! Measures: the numbers of an operation's arguments, each a plain number
//! or a formula over the part's parameters, with the tolerance it is to
//! be made to.
//!
//! A part has named [`Parameters`]; a formula like `width / 2 + 1` refers
//! to them by name. Formulas are resolved where the part is built, with
//! the parameters of the variant being built, so one program builds every
//! variant of a part.

use std::collections::BTreeMap;
use std::fmt;

use crate::world::Tolerance;

/// The values of a part's parameters, by name.
pub type Parameters = BTreeMap<String, f64>;

/// No parameters.
pub static NO_PARAMETERS: Parameters = BTreeMap::new();

/// A number of an operation's arguments: a plain number, or a formula
/// over the part's parameters; and how exactly what is made has to
/// keep to it.
///
/// It is entered as it is written: `10`, `=width / 2`, and with a
/// tolerance after it `10 ±0.1` (or `10 +-0.1`), `10 +0.2/-0.1`,
/// `10 H7`.
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    /// The number; for a formula, its value with the part's own
    /// parameters when it was entered (what dialogs and handles show).
    pub value: f64,
    /// The formula, if it is one, e.g. `"width / 2"`.
    pub formula: Option<String>,
    /// Its tolerance. Nothing of the geometry follows from it: it is
    /// said of the model, for its drawing.
    pub tolerance: Tolerance,
}

/// A number as it is entered: with a point, no more decimals than it
/// has.
fn plain(x: f64) -> String {
    format!("{x}")
}

/// A number of a tolerance as it was typed: with a point or a comma,
/// its minus sign of either kind.
fn typed(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".").replace('−', "-");
    let text = text.strip_prefix('+').unwrap_or(&text);
    text.parse::<f64>().ok().filter(|x| x.is_finite())
}

/// What is entered, apart: the number or formula, and the tolerance
/// written after it.
fn apart(text: &str) -> (&str, Tolerance) {
    // `10 ±0.1`, `10 +-0.1`, `10 +/-0.1`.
    for sign in ["±", "+/-", "+-"] {
        if let Some((body, rest)) = text.rsplit_once(sign)
            && let Some(by) = typed(rest)
            && !body.trim().is_empty()
        {
            return (body.trim_end(), Tolerance::Symmetric(by.abs()));
        }
    }
    // `10 +0.2/-0.1 limits`: deviations, written as limits of size.
    let trimmed = text.trim_end();
    for word in [" limits", " lim"] {
        if let Some(rest) = trimmed.strip_suffix(word)
            && let (body, Tolerance::Deviations { upper, lower }) = apart(rest)
        {
            return (body, Tolerance::Limits { upper, lower });
        }
    }
    let Some((body, last)) = text.rsplit_once(char::is_whitespace) else {
        return (text, Tolerance::General);
    };
    let said = body.trim().trim_start_matches('=').trim_end();
    if said.is_empty() || said.ends_with(['+', '-', '*', '/', '^', '(', ',']) {
        return (text, Tolerance::General);
    }
    // `10 +0.2/-0.1`: both deviations with their signs.
    let signed = |t: &str| t.starts_with(['+', '-', '−']);
    if let Some((upper, lower)) = last.split_once('/')
        && signed(upper)
        && signed(lower)
        && let (Some(a), Some(b)) = (typed(upper), typed(lower))
    {
        let (upper, lower) = (a.max(b), a.min(b));
        return (body.trim_end(), Tolerance::Deviations { upper, lower });
    }
    // `10 H7`: a tolerance class of ISO 286, a letter or two and its
    // grade.
    let letters = last.trim_end_matches(|c: char| c.is_ascii_digit());
    let grade = last.len() - letters.len();
    if (1..=2).contains(&letters.len())
        && (1..=2).contains(&grade)
        && letters.chars().all(|c| c.is_ascii_alphabetic())
    {
        return (body.trim_end(), Tolerance::Fit(last.to_owned()));
    }
    (text, Tolerance::General)
}

impl Measure {
    /// The plain number `value`.
    pub const fn new(value: f64) -> Self {
        Self {
            value,
            formula: None,
            tolerance: Tolerance::General,
        }
    }

    /// The same, to be kept within `tolerance`.
    #[must_use]
    pub fn within(mut self, tolerance: Tolerance) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// The same number or formula with the value `value`, its tolerance
    /// as it was: what dragging a handle makes of it.
    #[must_use]
    pub fn valued(&self, value: f64) -> Self {
        Self::new(value).within(self.tolerance.clone())
    }

    /// The formula `text`, valued with `parameters` (0 while it cannot be:
    /// resolving it says why).
    pub fn formula(text: impl Into<String>, parameters: &Parameters) -> Self {
        let text = text.into();
        Self {
            value: evaluate(&text, parameters).unwrap_or(0.0),
            formula: Some(text),
            tolerance: Tolerance::General,
        }
    }

    /// What the user entered: `text` as a number, or, starting with `=`
    /// or not a number, as a formula valued with `parameters`; with the
    /// tolerance written after it, if one is.
    pub fn parse(text: &str, parameters: &Parameters) -> Self {
        let (body, tolerance) = apart(text.trim());
        let body = body.trim();
        let measure = match body.parse::<f64>() {
            Ok(value) if value.is_finite() => Self::new(value),
            _ => Self::formula(body.trim_start_matches('=').trim(), parameters),
        };
        measure.within(tolerance)
    }

    /// Whether it is written as text — a formula, or with a tolerance —
    /// and no number alone.
    pub fn is_written(&self) -> bool {
        self.formula.is_some() || self.tolerance != Tolerance::General
    }

    /// The number with `parameters`: the plain number, or the formula's
    /// value. Fails for a formula that cannot be valued.
    pub fn resolve(&self, parameters: &Parameters) -> Result<f64, FormulaError> {
        match &self.formula {
            Some(formula) => evaluate(formula, parameters),
            None => Ok(self.value),
        }
    }

    /// Whether it is a plain number: handles can drag it.
    pub fn is_plain(&self) -> bool {
        self.formula.is_none()
    }
}

impl From<f64> for Measure {
    fn from(value: f64) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Measure {
    /// As entered: the number, or `=formula`, and its tolerance.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.formula {
            Some(formula) => write!(f, "={formula}")?,
            None => write!(f, "{}", self.value)?,
        }
        match &self.tolerance {
            Tolerance::General | Tolerance::Reference | Tolerance::Exact => Ok(()),
            Tolerance::Symmetric(by) => write!(f, " ±{}", plain(*by)),
            Tolerance::Deviations { upper, lower } | Tolerance::Limits { upper, lower } => {
                let signed = |x: f64| match x < 0.0 {
                    true => plain(x),
                    false => format!("+{}", plain(x)),
                };
                // (Nothing below is written `-0`: a deviation has its
                // sign.)
                let lower = match *lower == 0.0 {
                    true => "-0".to_owned(),
                    false => signed(*lower),
                };
                write!(f, " {}/{lower}", signed(*upper))?;
                match self.tolerance {
                    Tolerance::Limits { .. } => write!(f, " limits"),
                    _ => Ok(()),
                }
            }
            Tolerance::Fit(class) => write!(f, " {class}"),
        }
    }
}

/// A plain number is stored as a number; a formula with its last value,
/// a toleranced one with its tolerance.
impl serde::Serialize for Measure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        if !self.is_written() {
            return serializer.serialize_f64(self.value);
        }
        let toleranced = self.tolerance != Tolerance::General;
        let fields = 1 + usize::from(self.formula.is_some()) + usize::from(toleranced);
        let mut s = serializer.serialize_struct("Measure", fields)?;
        if let Some(formula) = &self.formula {
            s.serialize_field("formula", formula)?;
        }
        s.serialize_field("value", &self.value)?;
        if toleranced {
            s.serialize_field("tolerance", &self.tolerance)?;
        }
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for Measure {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Stored {
            Number(f64),
            Written {
                #[serde(default)]
                formula: Option<String>,
                value: f64,
                #[serde(default)]
                tolerance: Tolerance,
            },
        }
        Ok(match Stored::deserialize(deserializer)? {
            Stored::Number(value) => Measure::new(value),
            Stored::Written {
                formula,
                value,
                tolerance,
            } => Measure {
                value,
                formula,
                tolerance,
            },
        })
    }
}

/// Why a formula has no value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormulaError {
    /// It is not a formula: what was unexpected, and where.
    #[error("in {formula:?}: unexpected {found} at {at}")]
    Syntax {
        /// The formula.
        formula: String,
        /// What was found.
        found: String,
        /// The character position.
        at: usize,
    },
    /// It names a parameter the part does not have.
    #[error("in {formula:?}: no parameter {name:?}")]
    Unknown {
        /// The formula.
        formula: String,
        /// The name.
        name: String,
    },
    /// Its value is not a finite number (a division by zero, say).
    #[error("{formula:?} is not a finite number")]
    NotFinite {
        /// The formula.
        formula: String,
    },
}

/// The value of `formula` with `parameters`.
///
/// Formulas are sums, products and powers (`+ - * / ^`) of numbers,
/// parameter names, `pi`, and the functions `sqrt`, `abs`, `sin`, `cos`,
/// `tan` (of degrees), `min` and `max`, with parentheses.
pub fn evaluate(formula: &str, parameters: &Parameters) -> Result<f64, FormulaError> {
    let mut parser = Parser {
        text: formula,
        at: 0,
        parameters,
    };
    let value = parser.sum()?;
    parser.skip_spaces();
    if parser.at < formula.len() {
        return Err(parser.unexpected());
    }
    if value.is_finite() {
        Ok(value)
    } else {
        Err(FormulaError::NotFinite {
            formula: formula.to_owned(),
        })
    }
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
    parameters: &'a Parameters,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.at..].chars().next()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    /// Takes `c` if it is next.
    fn take(&mut self, c: char) -> bool {
        self.skip_spaces();
        let found = self.peek() == Some(c);
        if found {
            self.at += c.len_utf8();
        }
        found
    }

    fn unexpected(&self) -> FormulaError {
        FormulaError::Syntax {
            formula: self.text.to_owned(),
            found: self
                .peek()
                .map_or_else(|| "the end".to_owned(), |c| format!("{c:?}")),
            at: self.at,
        }
    }

    fn sum(&mut self) -> Result<f64, FormulaError> {
        let mut value = self.product()?;
        loop {
            if self.take('+') {
                value += self.product()?;
            } else if self.take('-') {
                value -= self.product()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn product(&mut self) -> Result<f64, FormulaError> {
        let mut value = self.power()?;
        loop {
            if self.take('*') {
                value *= self.power()?;
            } else if self.take('/') {
                value /= self.power()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn power(&mut self) -> Result<f64, FormulaError> {
        if self.take('-') {
            return Ok(-self.power()?);
        }
        let base = self.atom()?;
        if self.take('^') {
            return Ok(base.powf(self.power()?));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<f64, FormulaError> {
        self.skip_spaces();
        if self.take('(') {
            let value = self.sum()?;
            return if self.take(')') {
                Ok(value)
            } else {
                Err(self.unexpected())
            };
        }
        let rest = &self.text[self.at..];
        let Some(first) = rest.chars().next() else {
            return Err(self.unexpected());
        };
        if first.is_ascii_digit() || first == '.' {
            // A number, with an exponent if one follows.
            let mut end = rest
                .find(|c: char| !c.is_ascii_digit() && c != '.')
                .unwrap_or(rest.len());
            let exponent = &rest[end..];
            if exponent.starts_with(['e', 'E']) {
                let digits = exponent[1..].trim_start_matches(['+', '-']);
                let n = digits
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(digits.len());
                if n > 0 {
                    end += exponent.len() - digits.len() + n;
                }
            }
            let Ok(value) = rest[..end].parse::<f64>() else {
                return Err(self.unexpected());
            };
            self.at += end;
            return Ok(value);
        }
        if !(first.is_alphabetic() || first == '_') {
            return Err(self.unexpected());
        }
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let name = &rest[..end];
        self.at += end;
        if self.take('(') {
            let mut arguments = vec![self.sum()?];
            while self.take(',') {
                arguments.push(self.sum()?);
            }
            if !self.take(')') {
                return Err(self.unexpected());
            }
            let one = arguments[0];
            return match (name, arguments.len()) {
                ("sqrt", 1) => Ok(one.sqrt()),
                ("abs", 1) => Ok(one.abs()),
                ("sin", 1) => Ok(one.to_radians().sin()),
                ("cos", 1) => Ok(one.to_radians().cos()),
                ("tan", 1) => Ok(one.to_radians().tan()),
                ("min", n) if n >= 1 => Ok(arguments.into_iter().fold(f64::INFINITY, f64::min)),
                ("max", n) if n >= 1 => Ok(arguments.into_iter().fold(f64::NEG_INFINITY, f64::max)),
                _ => Err(FormulaError::Unknown {
                    formula: self.text.to_owned(),
                    name: format!("{name}()"),
                }),
            };
        }
        if let Some(&value) = self.parameters.get(name) {
            return Ok(value);
        }
        if name == "pi" {
            return Ok(std::f64::consts::PI);
        }
        Err(FormulaError::Unknown {
            formula: self.text.to_owned(),
            name: name.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters() -> Parameters {
        Parameters::from([("width".to_owned(), 8.0), ("n".to_owned(), 3.0)])
    }

    #[test]
    fn formulas_are_valued_with_the_parameters() {
        let value = |formula: &str| evaluate(formula, &parameters());
        assert_eq!(value("width / 2 + 1"), Ok(5.0));
        assert_eq!(value("-(n - 1) ^ 2 * 2"), Ok(-8.0));
        assert_eq!(value("max(width, n, 10) - min(2, 1e1)"), Ok(8.0));
        assert!((value("sin(30) + sqrt(4)").unwrap() - 2.5).abs() < 1e-12);
        assert!(
            matches!(value("height * 2"), Err(FormulaError::Unknown { name, .. }) if name == "height")
        );
        assert!(matches!(
            value("width +"),
            Err(FormulaError::Syntax { at: 7, .. })
        ));
        assert!(matches!(value("width 2"), Err(FormulaError::Syntax { .. })));
        assert!(matches!(
            value("1 / (n - 3)"),
            Err(FormulaError::NotFinite { .. })
        ));
    }

    #[test]
    fn measures_are_numbers_or_formulas() {
        let p = parameters();
        assert_eq!(Measure::parse(" 2.5 ", &p), Measure::new(2.5));
        let half = Measure::parse("= width / 2", &p);
        assert_eq!(
            (half.value, half.formula.as_deref()),
            (4.0, Some("width / 2"))
        );
        assert_eq!(half.to_string(), "=width / 2");
        // With another variant's parameters, another value.
        let wide = Parameters::from([("width".to_owned(), 20.0)]);
        assert_eq!(half.resolve(&wide), Ok(10.0));
        assert_eq!(Measure::new(3.0).resolve(&wide), Ok(3.0));
        assert!(Measure::parse("depth", &p).resolve(&p).is_err());
    }

    #[test]
    fn measures_are_entered_with_their_tolerances() -> Result<(), serde_json::Error> {
        let p = parameters();
        let entered = |text: &str| Measure::parse(text, &p);
        let fit = Tolerance::Fit("H7".into());
        let deviations = Tolerance::Deviations {
            upper: 0.2,
            lower: -0.1,
        };
        for (text, value, tolerance, written) in [
            ("10 ±0.1", 10.0, Tolerance::Symmetric(0.1), "10 ±0.1"),
            ("10+-0,1", 10.0, Tolerance::Symmetric(0.1), "10 ±0.1"),
            (
                "=width/2 +/- 0.05",
                4.0,
                Tolerance::Symmetric(0.05),
                "=width/2 ±0.05",
            ),
            ("10 +0.2/-0.1", 10.0, deviations.clone(), "10 +0.2/-0.1"),
            ("10 -0.1/+0.2", 10.0, deviations, "10 +0.2/-0.1"),
            // (Deviations, written on a drawing as the two limits.)
            (
                "10 +0.2/-0.1 limits",
                10.0,
                Tolerance::Limits {
                    upper: 0.2,
                    lower: -0.1,
                },
                "10 +0.2/-0.1 limits",
            ),
            ("6 H7", 6.0, fit.clone(), "6 H7"),
            ("width H7", 8.0, fit, "=width H7"),
            // Sums stay sums, and a parameter is no fit.
            ("10 + 0.2", 10.2, Tolerance::General, "=10 + 0.2"),
            ("width * n", 24.0, Tolerance::General, "=width * n"),
            ("width + n", 11.0, Tolerance::General, "=width + n"),
            ("2.5", 2.5, Tolerance::General, "2.5"),
        ] {
            let measure = entered(text);
            assert!((measure.value - value).abs() < 1e-12, "{text}");
            assert_eq!(measure.tolerance, tolerance, "{text}");
            assert_eq!(measure.to_string(), written, "{text}");
            // As written it is entered again, and kept as it is.
            assert_eq!(entered(written), measure, "{text}");
            let kept = serde_json::to_string(&measure)?;
            assert_eq!(serde_json::from_str::<Measure>(&kept)?, measure, "{text}");
        }
        // A plain number is kept as one; dragging keeps the tolerance.
        assert_eq!(serde_json::to_string(&Measure::new(2.0))?, "2.0");
        assert_eq!(entered("6 H7").valued(7.0).to_string(), "7 H7");
        Ok(())
    }
}
