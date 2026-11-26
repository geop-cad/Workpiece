//! Operations that place other parts, one per file.

mod add_part;
pub mod assembly;
pub mod connector;
mod cut_parts;
mod drive;
mod mate;
mod mechanism;
mod mirror_part;
mod pattern_part;
pub mod placing;
mod weld;

pub use add_part::{AddPart, AddPartArgs, PART_EXTENSION};
pub use cut_parts::{CutParts, CutPartsArgs};
pub use drive::{Drive, DriveArgs};
pub use mate::{Anchor, Mate, MateArgs, joint_fields};
pub use mechanism::{Mechanism, MechanismArgs};
pub use mirror_part::{MirrorPart, MirrorPartArgs};
pub use pattern_part::{PatternPart, PatternPartArgs};
pub use placing::{Hold, Mating, constraint_label, joint_name};
pub use weld::{Weld, WeldArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<AddPart>("part");
    r.kind::<Mate>("joint");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Add part",
        icon: "part",
        key: 'I',
        hint: "Add a part to this one: a part of this project, or a STEP file. Drag it where it goes, or mate it: a reference on what is there, one on the part, and the constraint between them; as many mates as it takes",
        small: false,
        make: |cx| AddPart { id: (cx.fresh_id)("part"), args: AddPartArgs { hold: Hold::to_pick(), ..Default::default() } }.into(),
    });
    r.kind::<Mechanism>("mechanism");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Mechanism",
        icon: "mechanism",
        key: 'M',
        hint: "Tie two joints together: gears at a ratio, a pinion on a rack, a screw, slides that travel together. Point at the joints in the view",
        small: false,
        make: |cx| Mechanism::new((cx.fresh_id)("mechanism")).into(),
    });
    r.kind::<PatternPart>("pattern");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Pattern part",
        icon: "pattern",
        key: 'R',
        hint: "Repeat a placed part, each a step (a move, a turn) from the one before; the copies go where the first goes",
        small: true,
        make: |cx| PatternPart { id: (cx.fresh_id)("row"), args: Default::default() }.into(),
    });
    r.kind::<MirrorPart>("mirror");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Mirror part",
        icon: "mirror",
        key: 'm',
        hint: "Place the mirror image of a placed part in a plane; it goes where the part goes",
        small: true,
        make: |cx| {
            MirrorPart {
                id: (cx.fresh_id)("mirror"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.kind::<Drive>("drive");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Drive",
        icon: "drive",
        key: 'd',
        hint: "Hold a joint at an angle or a distance along its travel; with a parameter, the part's variants run it through its range",
        small: true,
        make: |cx| Drive::new((cx.fresh_id)("drive")).into(),
    });
    r.kind::<CutParts>("cutparts");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Cut parts",
        icon: "cutparts",
        key: 'u',
        hint: "Cut bodies of the assembly out of every placed part they run into: a hole through several parts at once",
        small: true,
        make: |cx| CutParts { id: (cx.fresh_id)("cut"), args: Default::default() }.into(),
    });
    r.kind::<Weld>("weld");
    r.tool(Tool {
        tab: "Assembly",
        group: "",
        label: "Weld",
        icon: "weld",
        key: 'w',
        hint: "Lay a weld bead along edges where placed parts meet: a round about the edge, less what the parts take up",
        small: true,
        make: |cx| Weld { id: (cx.fresh_id)("weld"), args: Default::default() }.into(),
    });
    r.icon("cutparts", include_str!("../icons/cutparts.svg"));
    r.icon("drive", include_str!("../icons/drive.svg"));
    r.icon("joint", include_str!("../icons/joint.svg"));
    r.icon("mechanism", include_str!("../icons/mechanism.svg"));
    r.icon("mirror", include_str!("../icons/mirror.svg"));
    r.icon("part", include_str!("../icons/part.svg"));
    r.icon("pattern", include_str!("../icons/pattern.svg"));
    r.icon("slot", include_str!("../icons/slot.svg"));
    r.icon("thread", include_str!("../icons/thread.svg"));
    r.icon("weld", include_str!("../icons/weld.svg"));
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
    "body-all",
    "body-solo",
    "constraint-fixed",
    "constraint-free",
    "mate-add",
    "mate-flip",
    "mate-remove",
    "mech-gear",
    "mech-linear",
    "mech-rack",
    "mech-screw",
    "std-all",
    "std-bearing",
    "std-nut",
    "std-pin",
    "std-screw",
    "std-washer",
);
