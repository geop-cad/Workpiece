//! Operations that blend edges: fillet and chamfer.

mod fillet;

pub use fillet::{Chamfer, ChamferArgs, Fillet, FilletArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut shapecraft_core::Registry) {
    use shapecraft_core::Tool;
    r.kind::<Fillet>("fillet");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Fillet",
        icon: "fillet",
        key: 'L',
        hint: "Round edges: by radius, width or distance, as an arc, a conic or with continuous curvature",
        small: false,
        make: |cx| Fillet { id: (cx.fresh_id)("fillet"), args: Default::default() }.into(),
    });
    r.kind::<Chamfer>("chamfer");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Chamfer",
        icon: "chamfer",
        key: 'H',
        hint: "Cut edges off flat, by one distance or two",
        small: false,
        make: |cx| {
            Chamfer {
                id: (cx.fresh_id)("chamfer"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Modify",
        label: "Fillet",
        icon: "fillet",
        key: 'L',
        hint: "Round the corners of the sheet's outline",
        small: true,
        make: |cx| {
            Fillet {
                id: (cx.fresh_id)("fillet"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.icon("chamfer", include_str!("../icons/chamfer.svg"));
    r.icon("fillet", include_str!("../icons/fillet.svg"));
}
