//! Operations that sweep sketches into bodies: extrude, revolve, sweep and loft, rib.

mod extrude;
mod revolve;
mod rib;
mod swept_loft;

pub use extrude::{Extent, Extrude, ExtrudeArgs};
pub use revolve::{Revolve, RevolveArgs, drawn_axes};
pub use rib::{Rib, RibArgs};
pub use swept_loft::{Rail, Role, Section, SectionEnd, SweptLoft, SweptLoftArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<Extrude>("extrude");
    r.tool(Tool {
        tab: "Modeling",
        group: "Create",
        label: "Extrude",
        icon: "extrude",
        key: 'E',
        hint: "Sweep areas of a sketch along its normal into a solid: a new body, or joined to, cut from or intersected with the others",
        small: false,
        make: |cx| Extrude::of((cx.fresh_id)("extrude"), None, false).into(),
    });
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Create",
        label: "Extrude",
        icon: "extrude",
        key: 'E',
        hint: "Sweep areas of a sketch along its normal into a solid: the plate a sheet is bent from",
        small: false,
        make: |cx| Extrude::of((cx.fresh_id)("extrude"), None, false).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Create",
        label: "Extrude",
        icon: "extrude",
        key: 'E',
        hint: "Sweep the curves of a sketch along its normal into a surface, its ends open or closed",
        small: false,
        make: |cx| Extrude::of((cx.fresh_id)("surface"), None, true).into(),
    });
    r.kind::<Revolve>("revolve");
    r.tool(Tool {
        tab: "Modeling",
        group: "Create",
        label: "Revolve",
        icon: "revolve",
        key: 'R',
        hint: "Sweep areas of a sketch around an axis into a solid",
        small: false,
        make: |cx| Revolve::of((cx.fresh_id)("revolve"), None, false).into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Create",
        label: "Revolve",
        icon: "revolve",
        key: 'R',
        hint: "Sweep the curves of a sketch around an axis into a surface",
        small: false,
        make: |cx| Revolve::of((cx.fresh_id)("surface"), None, true).into(),
    });
    r.kind::<SweptLoft>("sweep");
    r.tool(Tool {
        tab: "Modeling",
        group: "Create",
        label: "Sweep / Loft",
        icon: "sweep",
        key: 'G',
        hint: "Sweep one section along a path (or coil it about an axis), or loft a solid through several sections — held at its ends (normal, tangent, curvature) and following guide curves",
        small: false,
        make: |cx| SweptLoft { id: (cx.fresh_id)("sweep"), args: Default::default() }.into(),
    });
    r.tool(Tool {
        tab: "Surfacing",
        group: "Create",
        label: "Sweep / Loft",
        icon: "sweep",
        key: 'G',
        hint: "Sweep a section along a path, or loft through several — sketches, faces' outlines, edges, curves left open — into a surface, held at its ends (G1, G2 to faces) and following guide curves",
        small: false,
        make: |cx| SweptLoft { id: (cx.fresh_id)("sweep"), args: SweptLoftArgs { surface: true, ..Default::default() } }.into(),
    });
    r.kind::<Rib>("rib");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Rib",
        icon: "rib",
        key: 'r',
        hint: "A thin wall from the curves of a sketch to the body beside them, in the sketch's plane",
        small: true,
        make: |cx| Rib { id: (cx.fresh_id)("rib"), args: Default::default() }.into(),
    });
    r.icon("extrude", include_str!("../icons/extrude.svg"));
    r.icon("flip", include_str!("../icons/flip.svg"));
    r.icon("revolve", include_str!("../icons/revolve.svg"));
    r.icon("rib", include_str!("../icons/rib.svg"));
    r.icon("sweep", include_str!("../icons/sweep.svg"));
    r.icon("rail-path", include_str!("../icons/rail-path.svg"));
    r.icon("rail-guide", include_str!("../icons/rail-guide.svg"));
}
