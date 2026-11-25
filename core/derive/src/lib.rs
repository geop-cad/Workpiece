//! `#[derive(Arguments)]`: what an operation's arguments refer to, and
//! the dialog that edits them, from the fields of the arguments.
//!
//! Every field whose type is a reference (`Reference<V>`, one in an
//! `Option`, or a `Vec` of them) is among what the arguments refer to.
//! Those, and the fields that are a `Measure`, a `bool`, an `f64`, a
//! `usize` or a `String`, get a field in the dialog, in the order they stand,
//! labelled by their name. Attributes say the rest:
//!
//! - `#[arg(label = "Open at")]`: what the field is called.
//! - `#[arg(required)]`: a list that has to hold something.
//! - `#[arg(empty = "All bodies")]`: a reference that may stay empty,
//!   and what it shows while it is.
//! - `#[arg(choice)]`: a field of a type that is a choice of a few (it
//!   has `Choices`), shown as one.
//! - `#[arg(field)]`: a field of another type there is a widget for
//!   (three measures, as X, Y and Z).
//! - `#[arg(nested)]`: a field whose type has arguments of its own:
//!   what it refers to is referred to.
//! - `#[arg(options = "One way|Both ways")]`: what the two states of a
//!   `bool` are called.
//! - `#[arg(flip)]`: a `bool` shown as the small button that turns
//!   something the other way.
//! - `#[arg(row)]`: the field stands beside the one before it.
//! - `#[arg(when = "self.two")]`: the field shows only while that holds.
//! - `#[arg(among = "WAYS")]`: a choice of only those of its kind that
//!   the slice holds.
//! - `#[arg(label_by = "self.by.label()")]`: what the field is called,
//!   where that depends on the arguments.
//! - `#[arg(one_of = "names(context)")]`: a name (`Option<String>`)
//!   chosen among those the expression gives; with `empty`, none may
//!   be chosen, which shows as that.
//! - `#[arg(with = "path::to::function")]`: the field's widgets are
//!   what `function(&arguments, context)` makes.
//! - `#[arg(hidden)]`: no field in the dialog.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type, parse_macro_input};

/// What is said of a field.
#[derive(Default)]
struct Said {
    label: Option<String>,
    required: bool,
    empty: Option<String>,
    choice: bool,
    nested: bool,
    hidden: bool,
    options: Option<String>,
    flip: bool,
    row: bool,
    when: Option<String>,
    among: Option<String>,
    label_by: Option<String>,
    with: Option<String>,
    one_of: Option<String>,
}

fn said(field: &syn::Field) -> syn::Result<Said> {
    let mut out = Said::default();
    for attribute in &field.attrs {
        if !attribute.path().is_ident("arg") {
            continue;
        }
        attribute.parse_nested_meta(|meta| {
            let text = |meta: &syn::meta::ParseNestedMeta<'_>| -> syn::Result<String> {
                Ok(meta.value()?.parse::<syn::LitStr>()?.value())
            };
            if meta.path.is_ident("label") {
                out.label = Some(text(&meta)?);
            } else if meta.path.is_ident("empty") {
                out.empty = Some(text(&meta)?);
            } else if meta.path.is_ident("required") {
                out.required = true;
            } else if meta.path.is_ident("choice") || meta.path.is_ident("field") {
                out.choice = true;
            } else if meta.path.is_ident("nested") {
                out.nested = true;
            } else if meta.path.is_ident("hidden") {
                out.hidden = true;
            } else if meta.path.is_ident("options") {
                out.options = Some(text(&meta)?);
            } else if meta.path.is_ident("when") {
                out.when = Some(text(&meta)?);
            } else if meta.path.is_ident("among") {
                out.among = Some(text(&meta)?);
            } else if meta.path.is_ident("label_by") {
                out.label_by = Some(text(&meta)?);
            } else if meta.path.is_ident("with") {
                out.with = Some(text(&meta)?);
            } else if meta.path.is_ident("one_of") {
                out.one_of = Some(text(&meta)?);
            } else if meta.path.is_ident("flip") {
                out.flip = true;
            } else if meta.path.is_ident("row") {
                out.row = true;
            } else {
                return Err(meta.error("not something to say of an argument"));
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// The last name of a type's path, and the types in its angle brackets.
fn named(ty: &Type) -> Option<(String, Vec<&Type>)> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    let inner = match &last.arguments {
        syn::PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|a| match a {
                syn::GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Some((last.ident.to_string(), inner))
}

/// Whether a type is a reference, or an `Option` or `Vec` of them.
fn refers(ty: &Type) -> bool {
    match named(ty) {
        Some((name, _)) if name == "Reference" => true,
        Some((name, inner)) if name == "Option" || name == "Vec" => {
            inner.first().is_some_and(|t| refers(t))
        }
        _ => false,
    }
}

/// Whether a type gets a field in the dialog by itself.
fn plain(ty: &Type) -> bool {
    matches!(named(ty), Some((name, _)) if ["Measure", "bool", "f64", "usize", "String"].contains(&name.as_str()))
}

/// `open_at` as a label: `Open at`.
fn label_of(name: &str) -> String {
    let spaced = name.replace('_', " ");
    let mut letters = spaced.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => spaced,
    }
}

/// See the crate's documentation.
#[proc_macro_derive(Arguments, attributes(arg))]
pub fn arguments(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return syn::Error::new_spanned(name, "arguments are a struct")
            .to_compile_error()
            .into();
    };
    let Fields::Named(fields) = &data.fields else {
        return syn::Error::new_spanned(name, "arguments have named fields")
            .to_compile_error()
            .into();
    };
    let (mut sources, mut sources_mut, mut widgets) = (Vec::new(), Vec::new(), Vec::new());
    for field in &fields.named {
        let said = match said(field) {
            Ok(said) => said,
            Err(e) => return e.to_compile_error().into(),
        };
        let Some(ident) = &field.ident else {
            continue;
        };
        if said.nested {
            sources.push(quote! {
                out.extend(::workpiece_core::Arguments::references(&self.#ident));
            });
            sources_mut.push(quote! {
                out.extend(::workpiece_core::Arguments::references_mut(&mut self.#ident));
            });
            continue;
        }
        let refers = refers(&field.ty);
        if refers {
            sources.push(quote! {
                ::workpiece_core::arguments::Field::sources(&self.#ident, &mut out);
            });
            sources_mut.push(quote! {
                ::workpiece_core::arguments::Field::sources_mut(&mut self.#ident, &mut out);
            });
        }
        let (row, when) = (said.row, said.when.clone());
        let when = match when.as_deref().map(syn::parse_str::<syn::Expr>) {
            Some(Ok(when)) => quote! { #when },
            Some(Err(e)) => return e.to_compile_error().into(),
            None => quote! { true },
        };
        // A field the arguments' own function makes the widgets of.
        if let Some(with) = &said.with {
            let with = match syn::parse_str::<syn::Path>(with) {
                Ok(with) => with,
                Err(e) => return e.to_compile_error().into(),
            };
            widgets.push(quote! {
                if #when {
                    let made: ::std::vec::Vec<::workpiece_core::Widget<Self>> = #with(self, context);
                    for (k, widget) in made.into_iter().enumerate() {
                        fields.push((#row && k == 0, widget));
                    }
                }
            });
            continue;
        }
        if said.hidden
            || !(refers
                || said.choice
                || said.among.is_some()
                || said.one_of.is_some()
                || plain(&field.ty))
        {
            continue;
        }
        let label = said.label.unwrap_or_else(|| label_of(&ident.to_string()));
        let label = match said.label_by.as_deref().map(syn::parse_str::<syn::Expr>) {
            Some(Ok(by)) => quote! { #by },
            Some(Err(e)) => return e.to_compile_error().into(),
            None => quote! { #label },
        };
        let (call, extra) = match said.among.as_deref().map(syn::parse_str::<syn::Expr>) {
            Some(Ok(among)) => (
                quote! { ::workpiece_core::arguments::among },
                quote! { &#among[..], },
            ),
            Some(Err(e)) => return e.to_compile_error().into(),
            None => (
                quote! { ::workpiece_core::arguments::Field::widget },
                quote! {},
            ),
        };
        let (call, extra) = match said.one_of.as_deref().map(syn::parse_str::<syn::Expr>) {
            Some(Ok(names)) => (
                quote! { ::workpiece_core::arguments::one_of },
                quote! { #names, },
            ),
            Some(Err(e)) => return e.to_compile_error().into(),
            None => (call, extra),
        };
        let required = said.required;
        let empty = match said.empty {
            Some(empty) => quote! { ::core::option::Option::Some(#empty) },
            None => quote! { ::core::option::Option::None },
        };
        let options = match said.options.as_deref().and_then(|o| o.split_once('|')) {
            Some((no, yes)) => quote! { ::core::option::Option::Some((#no, #yes)) },
            None => quote! { ::core::option::Option::None },
        };
        let flip = said.flip;
        widgets.push(quote! {
            if #when {
                let args = ::core::clone::Clone::clone(self);
                let widget = #call(
                    &self.#ident,
                    #extra
                    &::workpiece_core::arguments::FieldOptions {
                        label: #label,
                        required: #required,
                        empty: #empty,
                        options: #options,
                        flip: #flip,
                    },
                    context,
                    move |x| {
                        let mut new = ::core::clone::Clone::clone(&args);
                        new.#ident = x;
                        new
                    },
                );
                fields.push((#row, widget));
            }
        });
    }
    let (generics, types, wher) = input.generics.split_for_impl();
    quote! {
        impl #generics ::workpiece_core::Arguments for #name #types #wher {
            fn references(&self) -> ::std::vec::Vec<::workpiece_core::Source> {
                #[allow(unused_mut)]
                let mut out = ::std::vec::Vec::new();
                #(#sources)*
                out
            }

            fn references_mut(&mut self) -> ::std::vec::Vec<&mut ::workpiece_core::Source> {
                #[allow(unused_mut)]
                let mut out = ::std::vec::Vec::new();
                #(#sources_mut)*
                out
            }

            fn fields(
                &self,
                context: &::workpiece_core::EditContext<'_>,
            ) -> ::std::vec::Vec<::workpiece_core::Widget<Self>> {
                let _ = context;
                #[allow(unused_mut)]
                let mut fields = ::std::vec::Vec::new();
                #(#widgets)*
                ::workpiece_core::arguments::rows(fields)
            }
        }
    }
    .into()
}
