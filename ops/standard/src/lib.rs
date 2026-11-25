//! Standard parts: screws, nuts, washers and bearings by their sizes.

mod standard;

pub use standard::{
    Entry, Family, Filter, StandardKind, StandardPart, StandardPartArgs, catalogue, lengths,
    standard,
};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<StandardPart>("standard");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Standard part",
        icon: "standard",
        key: 'n',
        hint: "Place a screw, nut, washer or bearing out of the standards: search the catalogue or filter it by kind and size, pick what it goes onto and with what, then the constraint between the two",
        small: false,
        make: |cx| StandardPart { id: (cx.fresh_id)("standard"), args: Default::default() }.into(),
    });
    r.icon("standard", include_str!("../icons/standard.svg"));
}
