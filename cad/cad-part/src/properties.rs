//! Properties: what parts say of themselves besides their shape — a
//! part number, who made it, its status. A workspace says which
//! properties its parts have and what each is ([`Schema`]); a part
//! holds its values. Title blocks and tables read them.

use std::collections::BTreeMap;

/// The file a workspace keeps its properties' schema in, in its
/// directory.
pub const SCHEMA_FILE: &str = "properties.json";

/// What a property is.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// Any text.
    #[default]
    Text,
    /// A whole number.
    Integer,
    /// A number.
    Number,
    /// A day, written `2026-11-22`.
    Date,
    /// Yes or no.
    Boolean,
    /// One of these.
    Choice(Vec<String>),
}

impl Kind {
    /// What it is called.
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Text => "Text",
            Kind::Integer => "Whole number",
            Kind::Number => "Number",
            Kind::Date => "Date",
            Kind::Boolean => "Yes / no",
            Kind::Choice(_) => "One of",
        }
    }

    /// Why `value` is none of it, if it is not. (Nothing is of every
    /// kind: the property is not said.)
    pub fn check(&self, value: &str) -> Result<(), String> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(());
        }
        let ok = match self {
            Kind::Text => true,
            Kind::Integer => value.parse::<i64>().is_ok(),
            Kind::Number => value.parse::<f64>().is_ok_and(f64::is_finite),
            Kind::Date => is_date(value),
            Kind::Boolean => matches!(value, "yes" | "no"),
            Kind::Choice(of) => of.iter().any(|c| c == value),
        };
        match (ok, self) {
            (true, _) => Ok(()),
            (false, Kind::Integer) => Err(format!("{value:?} is no whole number")),
            (false, Kind::Number) => Err(format!("{value:?} is no number")),
            (false, Kind::Date) => Err(format!("{value:?} is no day: write it as 2026-11-22")),
            (false, Kind::Boolean) => Err(format!("{value:?} is neither yes nor no")),
            (false, Kind::Choice(of)) => Err(format!("{value:?} is none of {}", of.join(", "))),
            (false, Kind::Text) => Ok(()),
        }
    }
}

/// Whether `text` is a day, as `2026-11-22`.
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    let [year, month, day] = parts[..] else {
        return false;
    };
    let number = |t: &str, digits: usize| {
        (t.len() == digits && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.parse::<u32>().ok())
            .flatten()
    };
    let (Some(year), Some(month), Some(day)) = (number(year, 4), number(month, 2), number(day, 2))
    else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// A property parts have.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Property {
    /// What it is called.
    pub name: String,
    /// What it is.
    #[serde(default)]
    pub kind: Kind,
    /// What it is of a part that does not say it.
    #[serde(default)]
    pub default: String,
    /// Whether every part has to say it.
    #[serde(default)]
    pub required: bool,
}

/// The properties the parts of a workspace have.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Schema {
    /// The properties, in the order they are listed.
    pub properties: Vec<Property>,
}

impl Default for Schema {
    fn default() -> Self {
        let property = |name: &str, kind: Kind, default: &str| Property {
            name: name.to_owned(),
            kind,
            default: default.to_owned(),
            required: false,
        };
        let status = ["Draft", "In review", "Released"].map(str::to_owned);
        Self {
            properties: vec![
                property("Part number", Kind::Text, ""),
                property("Revision", Kind::Text, "A"),
                property("Author", Kind::Text, ""),
                property("Approved by", Kind::Text, ""),
                property("Date", Kind::Date, ""),
                property("Status", Kind::Choice(status.to_vec()), "Draft"),
            ],
        }
    }
}

impl Schema {
    /// The property called `name`.
    pub fn get(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }

    /// A name no property has yet.
    pub fn fresh_name(&self) -> String {
        (1..)
            .map(|n| match n {
                1 => "Property".to_owned(),
                n => format!("Property {n}"),
            })
            .find(|name| self.get(name).is_none())
            .expect("unbounded range")
    }

    /// What a part whose own values are `said` says of every property:
    /// its own value, or what the property is by default.
    pub fn values(&self, said: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut all: BTreeMap<String, String> = said.clone();
        for property in &self.properties {
            let value = all.entry(property.name.clone()).or_default();
            if value.trim().is_empty() {
                value.clone_from(&property.default);
            }
        }
        all
    }

    /// What is wrong with what a part says (`said`): what it has to say
    /// and does not, and what is not of its property's kind.
    pub fn problems(&self, said: &BTreeMap<String, String>) -> Vec<String> {
        let values = self.values(said);
        let mut problems = Vec::new();
        for property in &self.properties {
            let value = values.get(&property.name).map_or("", String::as_str);
            if property.required && value.trim().is_empty() {
                problems.push(format!("{} is not said", property.name));
            }
            if let Err(why) = property.kind.check(value) {
                problems.push(format!("{}: {why}", property.name));
            }
        }
        problems
    }

    /// The schema as the text of its file.
    pub fn to_text(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }

    /// The schema the text of a file holds.
    pub fn from_text(text: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_are_of_their_kinds_and_default_where_not_said() {
        let mut schema = Schema::default();
        schema.properties.push(Property {
            name: "Count".into(),
            kind: Kind::Integer,
            default: String::new(),
            required: true,
        });
        let mut said = BTreeMap::new();
        assert_eq!(schema.values(&said)["Revision"], "A");
        assert_eq!(schema.problems(&said), ["Count is not said"]);
        said.insert("Count".to_owned(), "three".to_owned());
        said.insert("Date".to_owned(), "2026-02-30".to_owned());
        assert_eq!(schema.problems(&said).len(), 2);
        said.insert("Count".to_owned(), "3".to_owned());
        said.insert("Date".to_owned(), "2026-02-28".to_owned());
        said.insert("Status".to_owned(), "Released".to_owned());
        assert!(schema.problems(&said).is_empty());
        let text = schema.to_text().expect("written");
        assert_eq!(Schema::from_text(&text).expect("read"), schema);
    }
}
