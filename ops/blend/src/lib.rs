//! Operations that blend edges: fillet and chamfer.

mod fillet;

pub use fillet::{Chamfer, ChamferArgs, Fillet, FilletArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<Fillet>("fillet");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Fillet",
        icon: "fillet",
        key: 'L',
        hint: "Round edges, of a solid or of a surface: by radius, width or distance, as an arc, a conic or with continuous curvature",
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
    // The same steps on the Surfacing tab: edges of a surface (between
    // two of its faces) blend as a solid's do.
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Fillet",
        icon: "fillet",
        key: 'f',
        hint: "Round edges, of a surface (between two of its faces) or of a solid: by radius, width or distance",
        small: true,
        make: |cx| Fillet { id: (cx.fresh_id)("fillet"), args: Default::default() }.into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Chamfer",
        icon: "chamfer",
        key: 'H',
        hint: "Cut edges off flat, of a surface (between two of its faces) or of a solid",
        small: true,
        make: |cx| {
            Chamfer {
                id: (cx.fresh_id)("chamfer"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.icon("chamfer", include_str!("../icons/chamfer.svg"));
    r.icon("fillet", include_str!("../icons/fillet.svg"));
    for (name, svg) in OPTION_ICONS {
        r.icon(name, svg);
    }
}

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../icons/", $name, ".svg")))),*]
    };
}

/// The icons of the options of these operations' choices.
const OPTION_ICONS: &[(&str, &str)] = icons!(
    "blend-distance",
    "blend-radius",
    "blend-width",
    "chamfer-equal",
    "chamfer-two",
    "section-circular",
    "section-conic",
    "section-curvature",
    "tangent-chain",
    "tangent-picked",
);
