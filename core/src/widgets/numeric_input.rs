use super::{MapFn, WidgetHandler, map_handler};
use crate::measure::{Measure, Parameters};
use crate::world::Tolerance;

/// A number field. One made for a [`Measure`] also takes formulas over
/// the part's parameters (entered as `=width / 2`) and a tolerance
/// after the number (`10 ±0.1`, `10 +0.2/-0.1`, `10 H7`).
pub struct NumericInput<M> {
    /// The caption.
    pub label: String,
    /// The current value.
    pub value: f64,
    /// The formula the value comes from, if it does.
    pub formula: Option<String>,
    /// The tolerance of the value, if the field takes one: a measure's.
    pub tolerance: Option<Tolerance>,
    /// What the field shows where that is no number alone: a formula,
    /// a number with its tolerance, as it is entered.
    pub written: Option<String>,
    /// The field takes the keyboard (all it shows selected, so what is
    /// typed replaces it) once for each new key it is given: as a
    /// dimension just placed has its value typed next.
    pub focus: Option<String>,
    /// The message for a value the user entered.
    pub on_change: WidgetHandler<f64, M>,
    /// The message for what the user entered that is no number alone
    /// — a formula, a tolerance — if the field takes that.
    pub on_text: Option<WidgetHandler<String, M>>,
}

impl<M: 'static> NumericInput<M> {
    /// A field showing `value`; entering `x` produces `on_change(x)`.
    pub fn new(
        label: impl Into<String>,
        value: f64,
        on_change: impl Fn(f64) -> M + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            value,
            formula: None,
            tolerance: None,
            written: None,
            focus: None,
            on_change: Box::new(on_change),
            on_text: None,
        }
    }

    /// A field showing `measure`; entering a number or a formula (valued
    /// with `parameters`), with or without a tolerance, produces
    /// `on_change` of the new measure. A number alone (a handle that is
    /// dragged) keeps the tolerance.
    pub fn measure(
        label: impl Into<String>,
        measure: &Measure,
        parameters: &Parameters,
        on_change: impl Fn(Measure) -> M + Clone + 'static,
    ) -> Self {
        let (number, parameters) = (on_change.clone(), parameters.clone());
        let was = measure.clone();
        Self {
            label: label.into(),
            value: measure.value,
            formula: measure.formula.clone(),
            tolerance: Some(measure.tolerance.clone()),
            written: measure.is_written().then(|| measure.to_string()),
            focus: None,
            on_change: Box::new(move |x| number(was.valued(x))),
            on_text: Some(Box::new(move |text: String| {
                on_change(Measure::parse(&text, &parameters))
            })),
        }
    }

    /// The same field, taking the keyboard once for `key` (see
    /// [`Self::focus`]).
    #[must_use]
    pub fn focused(mut self, key: impl Into<String>) -> Self {
        self.focus = Some(key.into());
        self
    }

    /// Gives the value the tolerance `tolerance`, the number or formula
    /// as it is. No message for a field that takes no tolerance.
    pub fn tolerate(&self, tolerance: &Tolerance) -> Option<M> {
        self.tolerance.as_ref()?;
        let measure = Measure {
            value: self.value,
            formula: self.formula.clone(),
            tolerance: tolerance.clone(),
        };
        self.on_text
            .as_ref()
            .map(|enter| enter(measure.to_string()))
    }

    /// Enters `value`. Non-finite values (NaN, infinity) are ignored: no
    /// message.
    pub fn set(&self, value: f64) -> Option<M> {
        value.is_finite().then(|| (self.on_change)(value))
    }

    /// Enters what the user typed: a number, or (where the field takes
    /// them) a formula, with or without a leading `=`, and a tolerance
    /// after it. What is typed is all there is: a number alone has no
    /// tolerance. No message for text the field does not take.
    pub fn enter(&self, text: &str) -> Option<M> {
        let text = text.trim();
        let Some(on_text) = &self.on_text else {
            return self.set(text.parse::<f64>().ok()?);
        };
        (!text.trim_start_matches('=').trim().is_empty()).then(|| on_text(text.to_owned()))
    }

    pub(crate) fn map_rc<N: 'static>(self, f: &MapFn<M, N>) -> NumericInput<N> {
        NumericInput {
            label: self.label,
            value: self.value,
            formula: self.formula,
            tolerance: self.tolerance,
            written: self.written,
            focus: self.focus,
            on_change: map_handler(self.on_change, f),
            on_text: self.on_text.map(|h| map_handler(h, f)),
        }
    }
}
