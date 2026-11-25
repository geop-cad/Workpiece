use shapecraft_cad_part::{Parameter, Variant};
use shapecraft_core::Parameters;

use crate::{Message, Model};

/// The active part's parameters and its table of variants, as shown and
/// edited. Edits replace the whole table
/// ([`Message::SetTable`](crate::Message::SetTable)).
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// The part's path.
    pub part: String,
    /// Its parameters, in order.
    pub parameters: Vec<Parameter>,
    /// Its variants.
    pub variants: Vec<Variant>,
    /// The variant the part is shown as.
    pub shown: Option<String>,
    /// The parameters' values as shown; `Err` says which formula has
    /// none.
    pub values: Result<Parameters, String>,
}

impl Table {
    /// The table of `model`'s active part.
    pub fn new(model: &Model) -> Option<Self> {
        let path = model.active_part.clone()?;
        let part = model.active_part()?;
        Some(Self {
            part: path,
            parameters: part.parameters.clone(),
            variants: part.variants.clone(),
            shown: model.variant.clone(),
            values: part
                .resolve(model.variant.as_deref())
                .map_err(|e| format!("{e:#}")),
        })
    }

    /// The message that stores the table as it is now.
    pub fn store(&self) -> Message {
        Message::SetTable {
            part: self.part.clone(),
            parameters: self.parameters.clone(),
            variants: self.variants.clone(),
        }
    }

    /// A name no parameter has yet: `prefix` and a number.
    pub fn fresh_parameter(&self, prefix: &str) -> String {
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|name| self.parameters.iter().all(|p| p.name != *name))
            .expect("unbounded range")
    }

    /// A name no variant has yet: `prefix` and a number.
    pub fn fresh_variant(&self, prefix: &str) -> String {
        (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|name| self.variants.iter().all(|v| v.name != *name))
            .expect("unbounded range")
    }
}
