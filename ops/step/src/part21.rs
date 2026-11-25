//! A reader for the exchange structure of STEP files (ISO 10303-21).
//!
//! A file is a list of instances, `#12 = NAME(arguments);`, that refer
//! to each other by number. Reading one is a single pass over the bytes
//! that only finds where each instance starts and ends; an instance's
//! arguments are parsed when something asks for them, straight from the
//! file's bytes, with nothing copied. A boundary representation uses a
//! small part of what a file holds (the rest is styling, annotation,
//! product structure), so most of a file is never parsed at all.

use std::collections::HashMap;

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SyntaxError {
    /// The file has no `DATA;` section.
    #[error("the file has no DATA section")]
    NoData,
    /// An instance does not end, or does not start with `#number =`.
    #[error("the instance at byte {0} is malformed")]
    Instance(usize),
}

/// A file, indexed: where each instance's text is.
pub struct File<'a> {
    bytes: &'a [u8],
    /// The instances in file order: number, and where what follows `=`
    /// starts and ends (before the `;`).
    instances: Vec<(u32, u32, u32)>,
    /// Where each number is in `instances`: a table for files numbered
    /// densely, a map otherwise.
    dense: Vec<u32>,
    sparse: HashMap<u32, u32>,
}

/// Skips blanks and comments from `at`.
fn skip(bytes: &[u8], mut at: usize) -> usize {
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes[at..].starts_with(b"/*") {
            at += 2;
            while at < bytes.len() && !bytes[at..].starts_with(b"*/") {
                at += 1;
            }
            at = (at + 2).min(bytes.len());
        } else {
            return at;
        }
    }
}

/// The end of the string that starts (with its quote) at `at`: the byte
/// after its closing quote. A doubled quote is a quote in the string.
fn string_end(bytes: &[u8], mut at: usize) -> usize {
    at += 1;
    while at < bytes.len() {
        if bytes[at] == b'\'' {
            if bytes.get(at + 1) == Some(&b'\'') {
                at += 2;
                continue;
            }
            return at + 1;
        }
        at += 1;
    }
    at
}

impl<'a> File<'a> {
    /// Indexes the instances of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Result<Self, SyntaxError> {
        let data = bytes
            .windows(5)
            .position(|w| w == b"DATA;")
            .ok_or(SyntaxError::NoData)?;
        let mut instances = Vec::with_capacity(bytes.len() / 48);
        let mut at = data + 5;
        let mut highest = 0;
        loop {
            at = skip(bytes, at);
            if at >= bytes.len() || bytes[at] != b'#' {
                // `ENDSEC;` ends the section.
                break;
            }
            let start = at;
            at += 1;
            let mut number: u32 = 0;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                number = number
                    .wrapping_mul(10)
                    .wrapping_add(u32::from(bytes[at] - b'0'));
                at += 1;
            }
            at = skip(bytes, at);
            if bytes.get(at) != Some(&b'=') {
                return Err(SyntaxError::Instance(start));
            }
            at = skip(bytes, at + 1);
            let from = at;
            // To the `;` that is not in a string.
            loop {
                match bytes.get(at) {
                    None => return Err(SyntaxError::Instance(start)),
                    Some(b';') => break,
                    Some(b'\'') => at = string_end(bytes, at),
                    Some(_) => at += 1,
                }
            }
            instances.push((number, from as u32, at as u32));
            highest = highest.max(number);
            at += 1;
        }
        let mut dense = Vec::new();
        let mut sparse = HashMap::new();
        if (highest as usize) < instances.len() * 8 + 1024 {
            dense = vec![u32::MAX; highest as usize + 1];
            for (k, i) in instances.iter().enumerate() {
                dense[i.0 as usize] = k as u32;
            }
        } else {
            sparse = instances
                .iter()
                .enumerate()
                .map(|(k, i)| (i.0, k as u32))
                .collect();
        }
        Ok(Self {
            bytes,
            instances,
            dense,
            sparse,
        })
    }

    /// How many instances the file has.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Whether it has none.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    fn text(&self, k: usize) -> &'a [u8] {
        let (_, from, to) = self.instances[k];
        &self.bytes[from as usize..to as usize]
    }

    /// The instance numbered `id`.
    pub fn get(&self, id: u32) -> Option<Instance<'a>> {
        let k = if self.dense.is_empty() {
            *self.sparse.get(&id)?
        } else {
            *self.dense.get(id as usize)?
        };
        (k != u32::MAX).then(|| Instance {
            id,
            text: self.text(k as usize),
        })
    }

    /// Every instance, in file order.
    pub fn instances(&self) -> impl Iterator<Item = Instance<'a>> + '_ {
        self.instances.iter().enumerate().map(|(k, i)| Instance {
            id: i.0,
            text: self.text(k),
        })
    }

    /// Every instance of type `name` (simple ones, and complex ones
    /// with a part of that type).
    pub fn of_type(&self, name: &'a str) -> impl Iterator<Item = Instance<'a>> + '_ {
        self.instances().filter(move |i| i.is(name))
    }
}

/// One instance: its number and its text, from the type name to the
/// closing parenthesis.
#[derive(Debug, Clone, Copy)]
pub struct Instance<'a> {
    /// Its number.
    pub id: u32,
    text: &'a [u8],
}

/// The end of the parenthesis that opens at `at`: the byte after its
/// closing one.
fn close(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return at + 1;
                }
            }
            b'\'' => {
                at = string_end(bytes, at);
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    at
}

fn name_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
        at += 1;
    }
    at
}

impl<'a> Instance<'a> {
    /// Its parts: one for a simple instance, several for a complex one
    /// (`(A(...) B(...))`). Each is a type name and its arguments.
    pub fn parts(&self) -> impl Iterator<Item = (&'a str, Arguments<'a>)> + 'a {
        let text = self.text;
        let complex = text.first() == Some(&b'(');
        let mut at = usize::from(complex);
        std::iter::from_fn(move || {
            at = skip(text, at);
            let end = name_end(text, at);
            if end == at {
                return None;
            }
            let name = std::str::from_utf8(&text[at..end]).ok()?;
            let open = skip(text, end);
            if text.get(open) != Some(&b'(') {
                return None;
            }
            let closed = close(text, open);
            let arguments = Arguments {
                text: &text[open + 1..closed.saturating_sub(1).max(open + 1)],
            };
            at = closed;
            Some((name, arguments))
        })
    }

    /// Whether it is (or, complex, has a part) of type `name`.
    pub fn is(&self, name: &str) -> bool {
        if self.text.first() != Some(&b'(') {
            let end = name_end(self.text, 0);
            return &self.text[..end] == name.as_bytes();
        }
        self.parts().any(|p| p.0 == name)
    }

    /// The type name of a simple instance (of a complex one, of its
    /// first part).
    pub fn name(&self) -> &'a str {
        self.parts().next().map_or("", |p| p.0)
    }

    /// The arguments of its part of type `name`.
    pub fn arguments(&self, name: &str) -> Option<Arguments<'a>> {
        self.parts().find(|p| p.0 == name).map(|p| p.1)
    }

    /// The arguments of a simple instance.
    pub fn own(&self) -> Arguments<'a> {
        self.parts().next().map_or(Arguments { text: b"" }, |p| p.1)
    }
}

/// A list of values: an instance's arguments, or a list among them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arguments<'a> {
    text: &'a [u8],
}

/// One value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    /// A reference to another instance, `#12`.
    Ref(u32),
    /// A whole number.
    Int(i64),
    /// A real number.
    Real(f64),
    /// A string, as written (quotes doubled, and so on).
    Str(&'a str),
    /// An enumeration value or logical, without its dots: `T`, `F`,
    /// `UNSPECIFIED`.
    Enum(&'a str),
    /// A list.
    List(Arguments<'a>),
    /// A value with its type written out, `LENGTH_MEASURE(1.0)`.
    Typed(&'a str, Arguments<'a>),
    /// `$`: not given.
    Unset,
    /// `*`: derived.
    Derived,
}

impl<'a> Arguments<'a> {
    /// The values, in order.
    pub fn values(&self) -> impl Iterator<Item = Value<'a>> + 'a {
        let text = self.text;
        let mut at = 0;
        std::iter::from_fn(move || {
            at = skip(text, at);
            if at >= text.len() {
                return None;
            }
            let start = at;
            let value = match text[at] {
                b'#' => {
                    at += 1;
                    let from = at;
                    while at < text.len() && text[at].is_ascii_digit() {
                        at += 1;
                    }
                    let n = std::str::from_utf8(&text[from..at]).ok()?.parse().ok()?;
                    Value::Ref(n)
                }
                b'\'' => {
                    at = string_end(text, at);
                    Value::Str(std::str::from_utf8(&text[start + 1..at - 1]).unwrap_or(""))
                }
                b'.' if text
                    .get(at + 1)
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') =>
                {
                    at += 1;
                    let from = at;
                    while at < text.len() && text[at] != b'.' {
                        at += 1;
                    }
                    let name = std::str::from_utf8(&text[from..at]).ok()?;
                    at += 1;
                    Value::Enum(name)
                }
                b'(' => {
                    at = close(text, at);
                    Value::List(Arguments {
                        text: &text[start + 1..at - 1],
                    })
                }
                b'$' => {
                    at += 1;
                    Value::Unset
                }
                b'*' => {
                    at += 1;
                    Value::Derived
                }
                c if c.is_ascii_alphabetic() => {
                    at = name_end(text, at);
                    let name = std::str::from_utf8(&text[start..at]).ok()?;
                    let open = skip(text, at);
                    at = close(text, open);
                    Value::Typed(
                        name,
                        Arguments {
                            text: &text[open + 1..at.saturating_sub(1).max(open + 1)],
                        },
                    )
                }
                _ => {
                    let mut real = false;
                    while at < text.len()
                        && !matches!(text[at], b',' | b')' | b' ' | b'\n' | b'\r' | b'\t')
                    {
                        real |= matches!(text[at], b'.' | b'E' | b'e');
                        at += 1;
                    }
                    let number = std::str::from_utf8(&text[start..at]).ok()?;
                    if real {
                        Value::Real(number.parse().ok()?)
                    } else {
                        Value::Int(number.parse().ok()?)
                    }
                }
            };
            // Past the comma.
            at = skip(text, at);
            if text.get(at) == Some(&b',') {
                at += 1;
            }
            Some(value)
        })
    }

    /// The value at `index`.
    pub fn at(&self, index: usize) -> Option<Value<'a>> {
        self.values().nth(index)
    }

    /// The reference at `index`.
    pub fn reference(&self, index: usize) -> Option<u32> {
        self.at(index)?.reference()
    }

    /// The number at `index`.
    pub fn real(&self, index: usize) -> Option<f64> {
        self.at(index)?.real()
    }

    /// The list at `index`.
    pub fn list(&self, index: usize) -> Option<Arguments<'a>> {
        match self.at(index)? {
            Value::List(l) => Some(l),
            _ => None,
        }
    }

    /// The logical at `index`: `.T.` or `.F.`.
    pub fn flag(&self, index: usize) -> Option<bool> {
        match self.at(index)? {
            Value::Enum("T") => Some(true),
            Value::Enum("F") => Some(false),
            _ => None,
        }
    }
}

impl<'a> Value<'a> {
    /// The instance it refers to.
    pub fn reference(&self) -> Option<u32> {
        match self {
            Value::Ref(n) => Some(*n),
            _ => None,
        }
    }

    /// It as a number: a real, a whole number, or a typed measure.
    pub fn real(&self) -> Option<f64> {
        match self {
            Value::Real(x) => Some(*x),
            Value::Int(n) => Some(*n as f64),
            Value::Typed(_, inner) => inner.real(0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"ISO-10303-21;
HEADER; FILE_NAME('a;b','',(''),(''),'','',''); ENDSEC;
DATA;
#1 = CARTESIAN_POINT('it''s; here',(0.,1.5,-2.E-1));
/* a comment; with #9 = X(); in it */
#2=DIRECTION('',(0.,0.,1.));
#30 = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
#4 = UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#30,'d',$);
#5 = EDGE_CURVE('',#1,#1,#2,.T.);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn instances_are_found_and_parsed_when_asked() -> Result<(), SyntaxError> {
        let file = File::new(SAMPLE)?;
        assert_eq!(file.len(), 5);
        let point = file.get(1).expect("it is there");
        assert_eq!(point.name(), "CARTESIAN_POINT");
        let args = point.own();
        assert_eq!(args.at(0), Some(Value::Str("it''s; here")));
        let xyz: Vec<f64> = args
            .list(1)
            .expect("coordinates")
            .values()
            .filter_map(|v| v.real())
            .collect();
        assert_eq!(xyz, [0.0, 1.5, -0.2]);
        // A complex instance has its parts.
        let unit = file.get(30).expect("it is there");
        assert!(unit.is("SI_UNIT") && unit.is("LENGTH_UNIT") && !unit.is("PLANE_ANGLE_UNIT"));
        let si = unit.arguments("SI_UNIT").expect("the part");
        assert_eq!(si.at(0), Some(Value::Enum("MILLI")));
        assert_eq!(si.at(1), Some(Value::Enum("METRE")));
        // Typed values, references, logicals, and what is not given.
        let measure = file.get(4).expect("it is there").own();
        assert_eq!(measure.real(0), Some(1e-7));
        assert_eq!(measure.reference(1), Some(30));
        assert_eq!(measure.at(3), Some(Value::Unset));
        let edge = file.get(5).expect("it is there").own();
        assert_eq!(edge.flag(4), Some(true));
        assert_eq!(file.of_type("DIRECTION").count(), 1);
        assert!(file.get(9).is_none());
        Ok(())
    }
}
