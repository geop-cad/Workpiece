//! Sheet metal: flanges bent up, and bent sheets laid out flat.

mod flange;

pub use flange::{Flange, FlangeArgs, Unfold, UnfoldArgs, flanged, flat_length, unfolded};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut shapecraft_core::Registry) {
    use shapecraft_core::Tool;
    r.kind::<Flange>("flange");
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Bend",
        label: "Flange",
        icon: "flange",
        key: 'G',
        hint: "Bend a flange up along a straight edge of a plate — a bend of a radius through an angle, then a flat flange; its summary tells the flat length",
        small: false,
        make: |cx| Flange { id: (cx.fresh_id)("flange"), args: Default::default() }.into(),
    });
    r.kind::<Unfold>("unfold");
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Bend",
        label: "Unfold",
        icon: "unfold",
        key: 'U',
        hint: "Lay a bent sheet out flat from one of its faces — its flat pattern, as a new body",
        small: false,
        make: |cx| {
            Unfold {
                id: (cx.fresh_id)("unfold"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.icon("flange", include_str!("../icons/flange.svg"));
    r.icon("unfold", include_str!("../icons/unfold.svg"));
}
