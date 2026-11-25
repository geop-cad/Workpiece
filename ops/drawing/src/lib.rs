//! Drawings of the model. A [`Drawing`] is a step of the part's
//! program: a sheet ([`Sheet`]) with views of the model as it is at
//! that step, on which what is said of the model — its dimensions,
//! datums and tolerances — is placed. [`lay::lay_out`] makes of it the
//! lines and the lettering of a drawing to ISO, for showing and as a
//! PDF file; [`quick::ThreeViews`] draws a model in three views with
//! nothing asked.

mod camera;
mod drawing;
pub mod lay;
pub mod quick;
mod section;
pub mod sheet;
pub mod template;

pub use camera::{CameraView, CameraViewArgs, Looking};
pub use drawing::Drawing;
pub use section::{Along, Section, SectionArgs, opened};
pub use sheet::Sheet;
pub use template::{Template, Templates};

/// Tells `r` of this crate's operation: its kind, the tool that adds
/// it, and its icon.
pub fn register(r: &mut workpiece_core::Registry) {
    use workpiece_core::Tool;
    r.kind::<Drawing>("drawing");
    r.tool(Tool {
        tab: "Annotate",
        group: "Drawing",
        label: "Drawing",
        icon: "drawing",
        key: 'W',
        hint: "A drawing of the model as it is here: a sheet with views, on which what is said of the model — its dimensions, datums and tolerances — is placed",
        small: false,
        make: |cx| {
            let id = (cx.fresh_id)("drawing");
            Drawing {
                args: Sheet::of(
                    cx.world,
                    cx.unit,
                    &id,
                    &Template::three_views("ISO A3", sheet::Format::A3, false),
                ),
                id,
            }
            .into()
        },
    });
    r.kind::<CameraView>("camera-view");
    r.kind::<Section>("section");
    r.tool(Tool {
        tab: "Annotate",
        group: "Views",
        label: "Camera view",
        icon: "camera-view",
        key: 'V',
        hint: "A named way of looking at the model — as the view is turned, from a side, or square onto a plane or a flat face — that the view flies to and drawings draw it from",
        small: true,
        make: |cx| {
            CameraView {
                id: (cx.fresh_id)("view"),
                args: CameraViewArgs::default(),
            }
            .into()
        },
    });
    r.tool(Tool {
        tab: "Annotate",
        group: "Views",
        label: "Section",
        icon: "section",
        key: 'X',
        hint: "Where the model is cut open to show its inside — along a plane, or along the lines of a sketch (an offset section): the view shows it cut so, and section views of drawings are of it",
        small: true,
        make: |cx| {
            Section {
                id: (cx.fresh_id)("section"),
                args: SectionArgs::default(),
            }
            .into()
        },
    });
    r.icon("drawing", include_str!("../icons/drawing.svg"));
    r.icon("camera-view", include_str!("../icons/camera-view.svg"));
    r.icon("section", include_str!("../icons/section.svg"));
    // (Of the panels that stand beside these tools.)
    r.icon("templates", include_str!("../icons/templates.svg"));
    r.icon("properties", include_str!("../icons/properties.svg"));
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
    "proj-parallel",
    "proj-perspective",
    "section-plane",
    "section-sketch",
    "view-current",
    "view-iso",
    "view-plane",
    "view-side",
);
