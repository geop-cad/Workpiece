//! An operation's arguments: what they refer to, and the fields that
//! edit them.
//!
//! [`Arguments`] is derived (`#[derive(Arguments)]`): the fields of the
//! arguments that are [references](Reference) are what the operation
//! refers to, and each field of a kind there is a widget for is a
//! field of the default dialog (see [`Field`]). An operation whose
//! dialog needs a layout of its own writes it itself.

use crate::measure::Measure;
use crate::operation::EditContext;
use crate::reference::{Reference, ReferenceKind, Refers, Source};
use crate::widgets::{Choice, NumericInput, ReferenceInput, ReferenceList, Widget};

/// What an operation is told to do.
pub trait Arguments: Clone + 'static {
    /// What they refer to.
    fn references(&self) -> Vec<Source>;

    /// The same, to change where they point (to follow a rename).
    fn references_mut(&mut self) -> Vec<&mut Source>;

    /// The fields that edit them, in order: each produces the arguments
    /// with what was entered.
    fn fields(&self, context: &EditContext<'_>) -> Vec<Widget<Self>>;
}

/// How a field shows in the dialog.
#[derive(Debug, Clone, Copy)]
pub struct FieldOptions {
    /// What it is called.
    pub label: &'static str,
    /// Whether a list has to hold something.
    pub required: bool,
    /// For a reference that may stay empty: what it shows while it is.
    pub empty: Option<&'static str>,
    /// What the two states of a yes-or-no are called: not so, and so.
    pub options: Option<(&'static str, &'static str)>,
    /// Whether a yes-or-no shows as the small button that turns
    /// something the other way.
    pub flip: bool,
}

/// The fields, one below the other, those that stand beside the one
/// before them (marked so) in a row with it.
pub fn rows<A: 'static>(fields: Vec<(bool, Widget<A>)>) -> Vec<Widget<A>> {
    let mut rows: Vec<Vec<Widget<A>>> = Vec::new();
    for (beside, field) in fields {
        match rows.last_mut() {
            Some(row) if beside => row.push(field),
            _ => rows.push(vec![field]),
        }
    }
    let one = |mut row: Vec<Widget<A>>| match row.len() {
        1 => row.remove(0),
        _ => crate::widgets::HBox::new(row).into(),
    };
    rows.into_iter().map(one).collect()
}

/// A value that is one of a few, to choose among: a kind, a way.
pub trait Choices: Copy + PartialEq + 'static {
    /// All of them, in the order they are offered.
    const ALL: &'static [Self];

    /// What this one is called.
    fn label(self) -> &'static str;
}

/// A type an argument can be of: what it refers to, and the widget
/// that edits it.
pub trait Field: Sized + Clone + 'static {
    /// What it refers to.
    fn sources(&self, out: &mut Vec<Source>) {
        let _ = out;
    }

    /// The same, to change.
    fn sources_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Source>) {
        let _ = out;
    }

    /// The widget that edits it: entering `x` produces `put(x)`.
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        context: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A>;
}

fn kind_of<V: Refers>() -> ReferenceKind {
    V::KINDS.first().copied().unwrap_or(ReferenceKind::Body)
}

impl<V: Refers + 'static> Field for Reference<V> {
    fn sources(&self, out: &mut Vec<Source>) {
        out.push(self.source.clone());
    }

    fn sources_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Source>) {
        out.push(&mut self.source);
    }

    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        let value = Some(self.source.clone());
        ReferenceInput::of_kind(options.label, kind_of::<V>(), value, move |s| {
            put(Reference::new(s))
        })
        .into()
    }
}

impl<V: Refers + 'static> Field for Option<Reference<V>> {
    fn sources(&self, out: &mut Vec<Source>) {
        out.extend(self.iter().map(|r| r.source.clone()));
    }

    fn sources_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Source>) {
        out.extend(self.iter_mut().map(|r| &mut r.source));
    }

    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        let value = self.as_ref().map(|r| r.source.clone());
        let pick = put.clone();
        let field = ReferenceInput::of_kind(options.label, kind_of::<V>(), value, move |s| {
            pick(Some(Reference::new(s)))
        });
        match options.empty {
            Some(empty) => field.optional(empty, move || put(None)).into(),
            None => field.into(),
        }
    }
}

impl<V: Refers + 'static> Field for Vec<Reference<V>> {
    fn sources(&self, out: &mut Vec<Source>) {
        out.extend(self.iter().map(|r| r.source.clone()));
    }

    fn sources_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Source>) {
        out.extend(self.iter_mut().map(|r| &mut r.source));
    }

    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        let values = self.iter().map(|r| r.source.clone()).collect();
        let list = ReferenceList::new(options.label, V::KINDS.to_vec(), values, move |all| {
            put(all.into_iter().map(Reference::new).collect())
        });
        if options.required {
            list.required().into()
        } else {
            list.into()
        }
    }
}

impl Field for Measure {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        context: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        NumericInput::measure(options.label, self, context.parameters, put).into()
    }
}

impl Field for f64 {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        NumericInput::new(options.label, *self, put).into()
    }
}

impl Field for usize {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        // (A count: what is entered, rounded, and not below nothing.)
        NumericInput::new(options.label, *self as f64, move |x| {
            put(x.round().max(0.0) as usize)
        })
        .into()
    }
}

impl Field for bool {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        if options.flip {
            let now = *self;
            let key = crate::Hotkey::key('X');
            return crate::widgets::ToolButton::new("flip", "Flip", key, move || put(!now))
                .with_hint("Go the other way")
                .into();
        }
        let (no, yes) = options.options.unwrap_or(("No", "Yes"));
        let options_of = vec![no.to_owned(), yes.to_owned()];
        Choice::new(options.label, options_of, usize::from(*self), move |i| {
            put(i == 1)
        })
        .into()
    }
}

impl<T: Choices + Clone> Field for T {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        let labels = T::ALL.iter().map(|c| c.label().to_owned()).collect();
        let chosen = T::ALL.iter().position(|c| c == self).unwrap_or(0);
        let first = *self;
        Choice::new(options.label, labels, chosen, move |i| {
            put(T::ALL.get(i).copied().unwrap_or(first))
        })
        .into()
    }
}

/// Three measures: along x, y and z, in a row.
impl Field for [Measure; 3] {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        context: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        // (Called as the label says, `Width|Depth|Height`, if it names
        // three.)
        let said: Vec<&str> = options.label.split('|').collect();
        let labels = <[&str; 3]>::try_from(said).unwrap_or(["X", "Y", "Z"]);
        let fields = labels.into_iter().enumerate().map(|(k, label)| {
            let (all, put) = (self.clone(), put.clone());
            NumericInput::measure(label, &self[k], context.parameters, move |x| {
                let mut new = all.clone();
                new[k] = x;
                put(new)
            })
            .into()
        });
        crate::widgets::HBox::new(fields.collect()).into()
    }
}

/// The widget that chooses `value` among `of`, those of its kind that
/// are offered here.
pub fn among<T: Choices, A: 'static>(
    value: &T,
    of: &[T],
    options: &FieldOptions,
    _: &EditContext<'_>,
    put: impl Fn(T) -> A + Clone + 'static,
) -> Widget<A> {
    let labels = of.iter().map(|c| c.label().to_owned()).collect();
    let chosen = of.iter().position(|c| c == value).unwrap_or(0);
    let (of, first) = (of.to_vec(), *value);
    Choice::new(options.label, labels, chosen, move |i| {
        put(of.get(i).copied().unwrap_or(first))
    })
    .into()
}

/// A text: a path, a name.
impl Field for String {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        crate::widgets::TextInput::new(options.label, self.clone(), put).into()
    }
}

/// A colour, or none.
impl Field for Option<[u8; 3]> {
    fn widget<A: 'static>(
        &self,
        options: &FieldOptions,
        _: &EditContext<'_>,
        put: impl Fn(Self) -> A + Clone + 'static,
    ) -> Widget<A> {
        crate::widgets::ColourInput::new(options.label, *self, put).into()
    }
}

/// The widget that chooses `value` among `names`; with what an empty
/// one shows as (`options.empty`), none of them may be chosen too.
pub fn one_of<A: 'static>(
    value: &Option<String>,
    names: Vec<String>,
    options: &FieldOptions,
    _: &EditContext<'_>,
    put: impl Fn(Option<String>) -> A + Clone + 'static,
) -> Widget<A> {
    let none = usize::from(options.empty.is_some());
    let chosen = value
        .as_ref()
        .and_then(|v| names.iter().position(|n| n == v));
    let mut labels: Vec<String> = options.empty.iter().map(|e| (*e).to_owned()).collect();
    labels.extend(names.iter().cloned());
    Choice::new(
        options.label,
        labels,
        chosen.map_or(0, |i| i + none),
        move |i| put(i.checked_sub(none).and_then(|i| names.get(i).cloned())),
    )
    .into()
}
