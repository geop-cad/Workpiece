//! The small icons beside the fields of a dialog: what a field is — a
//! distance, an angle, a count, a plane to pick — said by a picture as
//! well as by its name; and the pictures of the options of a choice.
//!
//! This is the one place that says which icon a field shows: by the
//! words of its label ([`label_icon`]), or by the kind of thing a
//! reference field picks ([`kind_icon`]). A field that wants another
//! says so itself (`with_icon`). The icons are drawn here, in
//! `core/icons/`, and told to the registry by [`register_icons`].

use crate::reference::ReferenceKind;

/// The icon of a field labelled `label`, by the words in it: "Head
/// diameter" shows the diameter's, "Angle °" the angle's. None where no
/// word says what the field is.
pub fn label_icon(label: &str) -> Option<&'static str> {
    let words: Vec<String> = label
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let has = |word: &str| words.iter().any(|w| w == word);
    // The more particular first: "Chamfer angle" is an angle, "Head
    // depth" a depth.
    BY_WORD
        .iter()
        .find(|(said, _)| said.split('|').any(has))
        .map(|(_, icon)| *icon)
}

/// Words of labels, and the icon each says.
const BY_WORD: &[(&str, &str)] = &[
    ("diameter|bore", "f-diameter"),
    ("radius|radii", "f-radius"),
    ("turns|revolutions", "f-turns"),
    ("angle|angles|turn|twist|draft", "f-angle"),
    ("pitch", "f-pitch"),
    ("depth|deep", "f-depth"),
    ("thickness|thick|wall", "f-thickness"),
    ("offset", "f-offset"),
    ("spacing|apart|gap|step", "f-spacing"),
    ("margin", "f-margin"),
    (
        "count|copies|number|instances|teeth|sides|segments",
        "f-count",
    ),
    ("distance|travel", "f-distance"),
    ("width|wide", "f-width"),
    ("height|high|tall", "f-height"),
    ("length|long", "f-length"),
    ("factor|scale|ratio", "f-scale"),
    ("size|sizes", "f-size"),
    ("clearance|fit|class", "f-fit"),
    ("tolerance", "f-fit"),
    ("extent|ends|end", "f-extent"),
    ("direction|way|towards", "f-direction"),
    ("placement|placed|position|place|seed", "f-position"),
    ("chamfer|deburr", "f-chamfer"),
    ("thread", "f-pitch"),
    ("name|text|title", "f-text"),
    ("colour|color", "f-colour"),
    ("material", "f-material"),
    ("letter", "f-text"),
    ("finer|level|levels|subdivisions", "f-level"),
    ("falloff", "f-falloff"),
    ("exposure|brightness", "f-exposure"),
    ("result|operation", "f-result"),
    ("construction|mode|kind", "f-mode"),
    ("corners|corner", "corner-round"),
    ("side", "side-both"),
    ("keep", "keep-copy"),
    ("hand", "hand-right"),
    ("gizmo", "gizmo-world"),
    ("measured|measure", "f-distance"),
];

/// The icon of a field that picks references of `kind`.
pub fn kind_icon(kind: ReferenceKind) -> &'static str {
    match kind {
        ReferenceKind::Vertex | ReferenceKind::Point => "f-point",
        ReferenceKind::Edge => "f-edge",
        ReferenceKind::Face => "f-face",
        ReferenceKind::Axis => "f-axis",
        ReferenceKind::Plane => "f-plane",
        ReferenceKind::Frame => "f-frame",
        ReferenceKind::Profile => "f-sketch",
        ReferenceKind::Body => "f-body",
    }
}

/// Tells `registry` of the icons of fields and of the options of
/// choices drawn here.
pub fn register_icons(registry: &mut crate::Registry) {
    for (name, svg) in ICONS {
        registry.icon(name, svg);
    }
}

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../../icons/", $name, ".svg")))),*]
    };
}

/// The icons, by name: `f-` those of fields, the others those of the
/// options of choices that many steps share.
const ICONS: &[(&str, &str)] = icons!(
    "corner-flat",
    "corner-miter",
    "corner-round",
    "corner-sharp",
    "ends-closed",
    "ends-open",
    "f-exposure",
    "f-falloff",
    "f-level",
    "f-mode",
    "f-result",
    "f-turns",
    "gizmo-normal",
    "joint-ball",
    "joint-cylindrical",
    "joint-fastened",
    "joint-planar",
    "joint-revolute",
    "joint-slider",
    "gizmo-world",
    "hand-left",
    "hand-right",
    "keep-copy",
    "keep-move",
    "side-back",
    "side-both",
    "side-front",
    "size-custom",
    "size-standard",
    "combine-cut",
    "combine-intersect",
    "combine-join",
    "combine-new",
    "extent-distance",
    "extent-face",
    "extent-next",
    "extent-symmetric",
    "extent-through",
    "extent-through-both",
    "f-angle",
    "f-axis",
    "f-body",
    "f-chamfer",
    "f-colour",
    "f-count",
    "f-depth",
    "f-diameter",
    "f-direction",
    "f-distance",
    "f-edge",
    "f-extent",
    "f-face",
    "f-fit",
    "f-frame",
    "f-height",
    "f-length",
    "f-margin",
    "f-material",
    "f-offset",
    "f-pitch",
    "f-plane",
    "f-point",
    "f-position",
    "f-radius",
    "f-scale",
    "f-size",
    "f-sketch",
    "f-spacing",
    "f-text",
    "f-thickness",
    "f-width",
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_say_their_icon() {
        assert_eq!(label_icon("Head diameter"), Some("f-diameter"));
        assert_eq!(label_icon("Chamfer angle"), Some("f-angle"));
        assert_eq!(label_icon("Angle °"), Some("f-angle"));
        assert_eq!(label_icon("Distance"), Some("f-distance"));
        assert_eq!(label_icon("Count"), Some("f-count"));
        assert_eq!(label_icon("Kind"), Some("f-mode"));
        assert_eq!(label_icon("Variant"), None);
        // Every icon a label or a kind says is drawn.
        let drawn: Vec<&str> = ICONS.iter().map(|(n, _)| *n).collect();
        for (_, icon) in BY_WORD {
            assert!(drawn.contains(icon), "{icon} is not drawn");
        }
    }
}
