//! Standard parts: screws, nuts, washers, pins and bearings by their sizes.

mod standard;

pub use standard::{
    Entry, Family, Filter, SEAT, StandardKind, StandardPart, StandardPartArgs, catalogue, lengths,
    seat_mate, standard,
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
        hint: "Place a screw, nut, washer, pin or bearing out of the standards: search the catalogue or filter it by kind and size, then point at what it goes onto: it sits there by its seat (a screw by the ring under its head). More mates hold it further",
        small: false,
        make: |cx| StandardPart::new((cx.fresh_id)("standard")).into(),
    });
    r.icon("standard", include_str!("../icons/standard.svg"));
}
