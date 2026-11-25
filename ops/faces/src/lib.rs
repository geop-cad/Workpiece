//! Operations on faces of bodies: shell, draft, thicken, move, emboss.
//! (Faces are deleted by Delete on the Surfacing tab: a surface's, or a
//! solid's, closed over where they were.)

mod draft;
mod emboss;
mod move_face;
mod shell;
mod thicken;

pub use draft::{Draft, DraftArgs};
pub use emboss::{Emboss, EmbossArgs, embossed};
pub use move_face::{FaceWay, MoveFace, MoveFaceArgs};
pub use shell::{Shell, ShellArgs};
pub use thicken::{Thicken, ThickenArgs, ThickenSide};
pub use workpiece_shape::heal::faces_deleted;

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<Thicken>("thicken");
    r.tool(Tool {
        tab: "Surfacing",
        group: "Create",
        label: "Thicken",
        icon: "thicken",
        key: 'k',
        hint: "Give faces or a whole surface a thickness: a solid of all within it of them",
        small: true,
        make: |cx| {
            Thicken {
                id: (cx.fresh_id)("thicken"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Modeling",
        group: "Create",
        label: "Thicken",
        icon: "thicken",
        key: 'k',
        hint: "Give faces or a whole surface a thickness: a solid of all within it of them, to one side or both",
        small: true,
        make: |cx| Thicken { id: (cx.fresh_id)("thicken"), args: Default::default() }.into(),
    });
    r.kind::<Shell>("shell");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Shell",
        icon: "shell",
        key: 'W',
        hint: "Hollow a body to a wall, open at the faces picked",
        small: true,
        make: |cx| {
            Shell {
                id: (cx.fresh_id)("shell"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.kind::<Draft>("draft");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Draft",
        icon: "draft",
        key: 'D',
        hint: "Lean faces by an angle about a neutral plane, as a moulded part needs them",
        small: true,
        make: |cx| {
            Draft {
                id: (cx.fresh_id)("draft"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.kind::<Emboss>("emboss");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Emboss",
        icon: "emboss",
        key: 'e',
        hint: "Raise the areas of a sketch from faces of a body, or sink them in (engrave), following the faces",
        small: true,
        make: |cx| Emboss { id: (cx.fresh_id)("emboss"), args: Default::default() }.into(),
    });
    r.kind::<MoveFace>("moveface");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Move Face",
        icon: "moveface",
        key: 'm',
        hint: "Push or pull faces of a body: off themselves (a hole widened, a boss thickened) or along an axis; the body follows",
        small: true,
        make: |cx| MoveFace { id: (cx.fresh_id)("moveface"), args: Default::default() }.into(),
    });
    r.icon("draft", include_str!("../icons/draft.svg"));
    r.icon("emboss", include_str!("../icons/emboss.svg"));
    r.icon("moveface", include_str!("../icons/moveface.svg"));
    r.icon("offset", include_str!("../icons/offset.svg"));
    r.icon("shell", include_str!("../icons/shell.svg"));
    r.icon("thicken", include_str!("../icons/thicken.svg"));
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
    "draft-both",
    "draft-one",
    "emboss-raised",
    "emboss-sunk",
    "moveface-along",
    "moveface-offset",
    "moveface-plane",
);
