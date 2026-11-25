//! Operations on whole bodies: combine, split, transform; and pattern
//! and mirror, of features or of bodies.

mod arrange;
mod combine;
mod delete;
mod features;
pub mod fill;
mod mirror;
mod pattern;
mod split;

pub use arrange::{Along, Motion, Transform, TransformArgs, along_curves, repeated};
pub use combine::{CombineArgs, CombineBodies};
pub use delete::{DeleteBody, DeleteBodyArgs};
pub use fill::Grid;
pub use mirror::{Mirror, MirrorArgs};
pub use pattern::{Pattern, PatternArgs, Repeat};
pub use split::{Keep, Split, SplitArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<CombineBodies>("combine");
    r.tool(Tool {
        tab: "Modeling",
        group: "Bodies",
        label: "Combine",
        icon: "combine",
        key: 'J',
        hint: "Join two bodies, cut one from the other, or keep what they share",
        small: false,
        make: |cx| {
            CombineBodies {
                id: (cx.fresh_id)("combine"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.kind::<Transform>("transform");
    r.tool(Tool {
        tab: "Modeling",
        group: "Bodies",
        label: "Transform",
        icon: "transform",
        key: 'T',
        hint: "Move, turn, mirror or scale a body, or a copy of it",
        small: true,
        make: |cx| {
            Transform {
                id: (cx.fresh_id)("transform"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.kind::<Pattern>("pattern");
    r.tool(Tool {
        tab: "Modeling",
        group: "Repeat",
        label: "Pattern",
        icon: "pattern",
        key: 'N',
        hint: "Repeat features — holes, bosses, pockets, fillets: what steps did to a body — or whole bodies, along a line, around an axis, along a path, at the points of a sketch, or filling a face with a grid or rings",
        small: false,
        make: |cx| Pattern { id: (cx.fresh_id)("pattern"), args: Default::default() }.into(),
    });
    r.kind::<Mirror>("mirror");
    r.tool(Tool {
        tab: "Modeling",
        group: "Repeat",
        label: "Mirror",
        icon: "mirror",
        key: 'i',
        hint: "Mirror features — holes, bosses, pockets, fillets: what steps did to a body — or whole bodies in a datum plane or a flat face, so a symmetric part needs each feature once",
        small: false,
        make: |cx| Mirror { id: (cx.fresh_id)("mirror"), args: Default::default() }.into(),
    });
    r.kind::<Split>("split");
    r.tool(Tool {
        tab: "Modeling",
        group: "Bodies",
        label: "Split",
        icon: "split",
        key: 'x',
        hint: "Cut a body in two by a plane, a flat face, or faces of any shape that go all the way through it (a surface), keeping both sides or one",
        small: true,
        make: |cx| {
            Split {
                id: (cx.fresh_id)("split"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.kind::<DeleteBody>("deletebody");
    r.tool(Tool {
        tab: "Modeling",
        group: "Bodies",
        label: "Delete body",
        icon: "deletebody",
        key: '\0',
        hint: "Take bodies out of the part: a tool body, a half a split left over, a surface that was only there to cut by",
        small: true,
        make: |cx| {
            DeleteBody {
                id: (cx.fresh_id)("delete"),
                args: Default::default(),
            }
            .into()
        },
    });
    r.icon("deletebody", include_str!("../icons/deletebody.svg"));
    r.icon("combine", include_str!("../icons/combine.svg"));
    r.icon("pattern", include_str!("../icons/pattern.svg"));
    r.icon("split", include_str!("../icons/split.svg"));
    r.icon("transform", include_str!("../icons/transform.svg"));
    r.icon("mirror", include_str!("../icons/mirror.svg"));
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
    "along-circle",
    "along-fill",
    "along-line",
    "along-path",
    "along-points",
    "fill-hexagonal",
    "fill-polygon",
    "fill-rectangular",
    "fill-rings",
    "fill-square",
    "fill-staggered",
    "motion-mirror",
    "motion-move",
    "motion-rotate",
    "motion-scale",
    "path-turn",
    "path-upright",
    "repeat-bodies",
    "repeat-features",
    "split-behind",
    "split-both",
    "split-front",
);
