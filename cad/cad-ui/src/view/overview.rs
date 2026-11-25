use workpiece_core::widgets::Button;
use workpiece_core::world::topo::BodyKind;

use super::Reading;
use crate::{Message, Model};

/// One item of the overview, with a toggle to show or hide it.
pub struct Item {
    /// Its name.
    pub name: String,
    /// Whether it is shown.
    pub visible: bool,
    /// Whether it is hidden only because later steps use it.
    pub auto_hidden: bool,
    /// Shows or hides it.
    pub toggle: Button<Message>,
    /// Shows it alone among the bodies (or all of them again).
    pub isolate: Button<Message>,
    /// The colour it shows in, if it has one (a body's own, or that of
    /// what it is made of).
    pub colour: Option<[u8; 3]>,
    /// What it is made of (a body), by the material's name.
    pub material: Option<String>,
}

/// What the focused part is made of, as shown: its bodies, sketches and
/// datums. Datums later steps use are hidden unless the user shows them.
pub struct Overview {
    /// The focused part's path.
    pub part: Option<String>,
    /// Its solid bodies (and its meshes).
    pub bodies: Vec<Item>,
    /// Its sheet bodies: faces that enclose nothing.
    pub faces: Vec<Item>,
    /// Its sketches (profiles).
    pub sketches: Vec<Item>,
    /// Its datums.
    pub datums: Vec<Item>,
    /// What is selected, measured.
    pub measured: Vec<Reading>,
    /// The unit of length the part's numbers are in.
    pub unit: workpiece_core::Unit,
    /// What the part is made of, where its steps say nothing else.
    pub material: Option<String>,
    /// The materials there are to choose from, by name.
    pub materials: Vec<String>,
    /// How the model is displayed: cut open, its parts apart.
    pub display: crate::model::display::Display,
    /// Whether parts are placed in it (which can be drawn apart).
    pub assembly: bool,
    /// Its joints that have limits: what can be run along its travel.
    pub joints: Vec<String>,
    /// What all that is placed in it measures together, and which
    /// placed parts run into each other.
    pub whole: Vec<Reading>,
    /// The parts placed in it, each (with its variant, if it is placed
    /// as one) with how many times: its bill of materials.
    pub placed: Vec<(String, usize)>,
}

impl Overview {
    /// The overview of `model`'s focused part.
    pub fn new(model: &Model) -> Self {
        let Some(part) = model.focused_part() else {
            return Self {
                part: None,
                bodies: Vec::new(),
                faces: Vec::new(),
                sketches: Vec::new(),
                datums: Vec::new(),
                measured: Vec::new(),
                unit: workpiece_core::Unit::default(),
                material: None,
                materials: Vec::new(),
                display: model.display.clone(),
                assembly: false,
                joints: Vec::new(),
                whole: Vec::new(),
                placed: Vec::new(),
            };
        };
        let world = &model.viewport.shown.world;
        let used = model.used_datums(&part);
        let library = &model.workspace.library;
        let bodies: std::collections::BTreeSet<&str> = world
            .topo
            .bodies()
            .ids()
            .filter_map(|b| world.topo.name_of(b))
            .collect();
        let item = |name: &str| {
            let (hidden, auto_hidden) = model.hidden(&part, name, &used);
            let message = Message::SetVisible {
                name: name.to_owned(),
                visible: hidden,
            };
            Item {
                name: name.to_owned(),
                visible: !hidden,
                auto_hidden,
                toggle: Button::new(if hidden { "Show" } else { "Hide" }, move || {
                    message.clone()
                }),
                isolate: {
                    let name = name.to_owned();
                    Button::new("Isolate", move || Message::Isolate(name.clone()))
                },
                colour: bodies
                    .contains(name)
                    .then(|| world.body_look(name))
                    .and_then(|look| {
                        let of = look.material.as_ref().and_then(|m| library.get(m));
                        look.colour.or(of.map(|m| m.colour))
                    }),
                material: bodies
                    .contains(name)
                    .then(|| world.body_look(name).material)
                    .flatten(),
            }
        };
        let topo = &world.topo;
        Self {
            bodies: topo
                .bodies()
                .iter()
                .filter(|(_, body)| body.kind != BodyKind::Sheet)
                .filter_map(|(b, _)| topo.name_of(b))
                .map(item)
                .collect(),
            faces: topo
                .bodies()
                .iter()
                .filter(|(_, body)| body.kind == BodyKind::Sheet)
                .filter_map(|(b, _)| topo.name_of(b))
                .map(item)
                .collect(),
            sketches: world.profiles.keys().map(|n| item(n)).collect(),
            datums: world.datums.keys().map(|n| item(n)).collect(),
            unit: model.unit(),
            material: model
                .workspace
                .part(&part)
                .ok()
                .and_then(|p| p.material.clone()),
            materials: library.names(),
            display: model.display.clone(),
            assembly: !world.instances.is_empty(),
            joints: crate::model::display::drivable(world),
            whole: super::inspect::of_assembly(world),
            measured: super::inspect::readings(
                &model.viewport.shown.world,
                &model.selection,
                model.density(),
            ),
            placed: {
                let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
                if let Ok(focused) = model.workspace.part(&part) {
                    for step in &focused.program {
                        if let Some((placed, variant)) = step.placed() {
                            let name = match variant {
                                Some(variant) => format!("{placed} ({variant})"),
                                None => placed.to_owned(),
                            };
                            *counts.entry(name).or_default() += 1;
                        }
                    }
                }
                counts.into_iter().collect()
            },
            part: Some(part),
        }
    }
}
