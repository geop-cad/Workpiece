//! How the model looks. An [`Appearance`] is a step that gives bodies
//! and faces a material or a colour of their own, over what the part
//! is made of; a [`Render`] is a step that is a picture of the model as
//! it is at that step, with light, shadow and reflections.

mod appearance;
mod render;

pub use appearance::{Appearance, AppearanceArgs};
pub use render::{Render, Shot, Sky, View};

/// Tells `r` of this crate's operations: their kinds, the tools that
/// add them, and their icons.
pub fn register(r: &mut shapecraft_core::Registry) {
    use shapecraft_core::Tool;
    r.kind::<Appearance>("appearance");
    r.kind::<Render>("render");
    r.tool(Tool {
        tab: "Properties",
        group: "Look",
        label: "Appearance",
        icon: "appearance",
        key: 'A',
        hint: "Give bodies or single faces a material or a colour of their own, over what the part is made of",
        small: false,
        make: |cx| {
            Appearance {
                id: (cx.fresh_id)("appearance"),
                args: AppearanceArgs::default(),
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Properties",
        group: "Picture",
        label: "Render",
        icon: "render",
        key: 'R',
        hint: "A picture of the model as it is here, with light, shadows and reflections: turn the view to what it shows, and save it as an image",
        small: false,
        make: |cx| {
            Render {
                id: (cx.fresh_id)("render"),
                args: Shot::default(),
            }
            .into()
        },
    });
    r.icon("appearance", include_str!("../icons/appearance.svg"));
    r.icon("render", include_str!("../icons/render.svg"));
    // (Of the panels that stand beside these tools.)
    r.icon("materials", include_str!("../icons/materials.svg"));
    r.icon("parameters", include_str!("../icons/parameters.svg"));
}
