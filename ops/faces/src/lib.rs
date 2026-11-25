//! Operations on faces of bodies: shell, draft, thicken, move, delete, emboss.

mod draft;
mod emboss;
mod heal;
mod move_face;
mod shell;
mod thicken;

pub use draft::{Draft, DraftArgs};
pub use emboss::{Emboss, EmbossArgs, embossed};
pub use heal::{DeleteFace, DeleteFaceArgs, faces_deleted};
pub use move_face::{FaceWay, MoveFace, MoveFaceArgs};
pub use shell::{Shell, ShellArgs};
pub use thicken::{Thicken, ThickenArgs, ThickenSide};

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
        group: "Direct",
        label: "Move Face",
        icon: "moveface",
        key: 'm',
        hint: "Push or pull faces of a body: off themselves (a hole widened, a boss thickened) or along an axis; the body follows",
        small: true,
        make: |cx| MoveFace { id: (cx.fresh_id)("moveface"), args: Default::default() }.into(),
    });
    r.kind::<DeleteFace>("deleteface");
    r.tool(Tool {
        tab: "Surfacing",
        group: "Edit",
        label: "Delete face",
        icon: "deleteface",
        key: 'd',
        hint: "Take the faces of a hole, a pocket or a boss off a body and close the faces around over where they were",
        small: true,
        make: |cx| DeleteFace { id: (cx.fresh_id)("deleteface"), args: Default::default() }.into(),
    });
    r.icon("deleteface", include_str!("../icons/deleteface.svg"));
    r.icon("draft", include_str!("../icons/draft.svg"));
    r.icon("emboss", include_str!("../icons/emboss.svg"));
    r.icon("moveface", include_str!("../icons/moveface.svg"));
    r.icon("offset", include_str!("../icons/offset.svg"));
    r.icon("shell", include_str!("../icons/shell.svg"));
    r.icon("thicken", include_str!("../icons/thicken.svg"));
}
