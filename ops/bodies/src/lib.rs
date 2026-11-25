//! Operations on whole bodies: combine, split, transform, pattern.

mod arrange;
mod combine;
mod delete;
mod feature_mirror;
mod feature_pattern;
mod features;
mod split;

pub use arrange::{
    Along, Motion, Pattern, PatternArgs, Transform, TransformArgs, along_curves, repeated,
};
pub use combine::{CombineArgs, CombineBodies};
pub use delete::{DeleteBody, DeleteBodyArgs};
pub use feature_mirror::{FeatureMirror, FeatureMirrorArgs};
pub use feature_pattern::{FeaturePattern, FeaturePatternArgs};
pub use split::{Keep, Split, SplitArgs};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut shapecraft_core::Registry) {
    use shapecraft_core::Tool;
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
    r.kind::<FeaturePattern>("pattern");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Feature pattern",
        icon: "pattern",
        key: 'n',
        hint: "Repeat features — what steps added to a body or took from it: holes, bosses, pockets, fillets, several at once — along a line, around an axis, along a path or at the points of a sketch",
        small: true,
        make: |cx| FeaturePattern { id: (cx.fresh_id)("featurepattern"), args: Default::default() }.into(),
    });
    r.kind::<FeatureMirror>("mirror");
    r.tool(Tool {
        tab: "Modeling",
        group: "Modify",
        label: "Feature mirror",
        icon: "mirror",
        key: 'i',
        hint: "Mirror features — what steps added to a body or took from it: holes, bosses, pockets, fillets, several at once — in a datum plane or a flat face, so a symmetric part needs each feature once",
        small: true,
        make: |cx| FeatureMirror { id: (cx.fresh_id)("featuremirror"), args: Default::default() }.into(),
    });
    r.kind::<Pattern>("pattern");
    r.tool(Tool {
        tab: "Modeling",
        group: "Bodies",
        label: "Pattern",
        icon: "pattern",
        key: 'N',
        hint: "Repeat a body along a line or around an axis: as bodies, joined into one, or cut out of another",
        small: true,
        make: |cx| Pattern { id: (cx.fresh_id)("pattern"), args: Default::default() }.into(),
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
}
