//! Features cut into bodies: holes and threads, to the standards'
//! sizes ([`norm`]) or to sizes of their own.

mod hole;
pub mod norm;
mod thread;

pub use hole::{Clearance, Deburr, Hole, HoleArgs, HoleKind, HolePlace, HoleShape, drill};
pub use thread::{Thread, ThreadArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<Hole>("hole");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Hole",
        icon: "hole",
        key: 'h',
        hint: "Drill holes — at the points of a sketch, off two planes, or around an axis: plain, counterbored, countersunk or tapped, for a screw size of the standards (M1 to M64) or to sizes of their own; to a depth or through, chamfered at either end",
        small: false,
        make: |cx| Hole { id: (cx.fresh_id)("hole"), args: Default::default() }.into(),
    });
    r.kind::<Thread>("thread");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Thread",
        icon: "thread",
        key: 't',
        hint: "Thread a cylindrical face, of a rod or of a hole: cosmetic (shown, and drawn as drawings show threads) or modelled as a V groove along a helix",
        small: true,
        make: |cx| Thread { id: (cx.fresh_id)("thread"), args: Default::default() }.into(),
    });
    r.tool(Tool {
        tab: "Sheet metal",
        group: "Modify",
        label: "Hole",
        icon: "hole",
        key: 'h',
        hint: "Drill holes at the points of a sketch, through the sheet or to a depth",
        small: true,
        make: |cx| {
            Hole {
                id: (cx.fresh_id)("hole"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.icon("hole", include_str!("../icons/hole.svg"));
    r.icon("thread", include_str!("../icons/thread.svg"));
}
