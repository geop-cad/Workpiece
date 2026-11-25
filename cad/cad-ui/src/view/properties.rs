use shapecraft_cad_part::Material;

use crate::{Model, Panel};

/// The materials of the library, as their panel shows them to be
/// changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Materials {
    /// The materials, in order.
    pub library: Vec<Material>,
    /// What the focused part is made of, if there is one and it says.
    pub of_part: Option<String>,
    /// Whether a part is focused: what can be made of one of them.
    pub part: bool,
    /// The file the library is kept in, in the working directory.
    pub file: &'static str,
}

impl Materials {
    /// The materials, while their panel shows.
    pub fn new(model: &Model) -> Option<Self> {
        if model.panel != Some(Panel::Materials) {
            return None;
        }
        let part = model
            .focused_part()
            .and_then(|path| model.workspace.part(&path).ok());
        Some(Self {
            library: model.workspace.library.materials.clone(),
            of_part: part.and_then(|p| p.material.clone()),
            part: part.is_some(),
            file: shapecraft_cad_part::LIBRARY_FILE,
        })
    }
}

/// What the focused part says of itself, and the properties parts
/// have, as their panel shows them to be changed.
#[derive(Debug, Clone, PartialEq)]
pub struct PartProperties {
    /// The properties parts have, in order.
    pub schema: Vec<shapecraft_cad_part::Property>,
    /// What the focused part says of them itself, by name.
    pub said: std::collections::BTreeMap<String, String>,
    /// What is wrong with what it says: what it has to say and does
    /// not, what is not of its property's kind.
    pub problems: Vec<String>,
    /// Whether a part is focused.
    pub part: bool,
    /// The unit of length the focused part's numbers are in.
    pub unit: shapecraft_core::Unit,
    /// The file the properties are kept in, in the working directory.
    pub file: &'static str,
}

impl PartProperties {
    /// The properties, while their panel shows.
    pub fn new(model: &Model) -> Option<Self> {
        if model.panel != Some(Panel::Properties) {
            return None;
        }
        let part = model
            .focused_part()
            .and_then(|path| model.workspace.part(&path).ok());
        let schema = &model.workspace.schema;
        Some(Self {
            schema: schema.properties.clone(),
            said: part.map(|p| p.properties.clone()).unwrap_or_default(),
            problems: part.map_or_else(Vec::new, |p| schema.problems(&p.properties)),
            part: part.is_some(),
            unit: model.unit(),
            file: shapecraft_cad_part::SCHEMA_FILE,
        })
    }
}

/// The sheet templates, as their panel shows them to be changed.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetTemplates {
    /// The templates, in order.
    pub templates: Vec<shapecraft_ops_drawing::Template>,
    /// The properties a cell of a title block can say: those every
    /// drawing has, and the parts' own.
    pub properties: Vec<String>,
    /// The camera views the focused part has: what a view of a
    /// template can be of, besides the sides.
    pub views: Vec<String>,
    /// The file the templates are kept in, in the working directory.
    pub file: &'static str,
}

impl SheetTemplates {
    /// The templates, while their panel shows.
    pub fn new(model: &Model) -> Option<Self> {
        if model.panel != Some(Panel::Templates) {
            return None;
        }
        Some(Self {
            templates: model.workspace.sheets.templates.clone(),
            properties: cell_properties(model),
            views: model.viewport.shown.world.views.keys().cloned().collect(),
            file: shapecraft_ops_drawing::template::TEMPLATES_FILE,
        })
    }
}

/// The properties a cell of a title block can say.
pub fn cell_properties(model: &Model) -> Vec<String> {
    let own = model.workspace.schema.properties.iter();
    shapecraft_ops_drawing::template::BUILT_IN
        .iter()
        .map(|name| (*name).to_owned())
        .chain(own.map(|p| p.name.clone()))
        .collect()
}

/// What a new drawing can start from, while that is asked.
#[derive(Debug, Clone, PartialEq)]
pub struct NewDrawing {
    /// The sheet templates, by name.
    pub templates: Vec<String>,
}

impl NewDrawing {
    /// What is asked, while it is.
    pub fn new(model: &Model) -> Option<Self> {
        (model.panel == Some(Panel::NewDrawing)).then(|| Self {
            templates: model.workspace.sheets.names(),
        })
    }
}
