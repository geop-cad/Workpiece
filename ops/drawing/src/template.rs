//! Sheet templates: what a new drawing starts from — the sheet's size,
//! the views it has, and how its title block looks. A workspace keeps
//! them in a library ([`Templates`]) beside its parts.

use crate::sheet::{Beside, Format, Projection, SheetView, Side, Style, ViewKind};

/// The file a workspace keeps its sheet templates in, in its directory.
pub const TEMPLATES_FILE: &str = "sheets.json";

/// What a cell of a title block says.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Content {
    /// Text of its own, entered on the sheet.
    Text(String),
    /// A property, by its name: one of the part's own (see the
    /// workspace's properties), or one of those every drawing has —
    /// `Name`, `Title` (the part's own, else its name), `Document type`
    /// (a part or an assembly drawing), `Material`, `Mass` (of what is
    /// placed in it too), `Scale`, `Format`, `Sheet`, `Projection` (the
    /// sign of how the views are arranged).
    Property(String),
}

/// A cell of a title block.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    /// Its lower left corner, in millimetres from the block's.
    pub at: [f64; 2],
    /// Its width and height.
    pub size: [f64; 2],
    /// What it is called: the small text in its upper left corner.
    pub label: String,
    /// What it says.
    pub content: Content,
    /// The height of what it says, in millimetres.
    pub height: f64,
}

/// The properties every drawing has, whatever the part's own are.
pub const BUILT_IN: [&str; 9] = [
    "Name",
    "Title",
    "Document type",
    "Material",
    "Mass",
    "Scale",
    "Format",
    "Sheet",
    "Projection",
];

/// What a new drawing starts from.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Template {
    /// What it is called.
    pub name: String,
    /// The sheet's size.
    pub format: Format,
    /// Whether the sheet stands upright.
    pub upright: bool,
    /// How the views are arranged about each other.
    pub projection: Projection,
    /// The views a drawing made from it starts with: where each stands
    /// on the sheet (what is projected keeps in line, as far from what
    /// it is projected from as the model is large).
    pub views: Vec<SheetView>,
    /// The title block: its cells. Its lower right corner is the
    /// drawing space's.
    pub block: Vec<Cell>,
    /// How dimensions stand off the outline and one another.
    #[serde(default)]
    pub spacing: crate::sheet::Spacing,
    /// Its line widths, lettering and centre lines (ISO 128-2, ISO
    /// 3098).
    #[serde(default)]
    pub drafting: crate::sheet::Drafting,
}

/// The title block of ISO 7200, 180 wide, with the row of what a
/// drawing is made to above it.
pub fn iso_block() -> Vec<Cell> {
    let text = |t: &str| Content::Text(t.to_owned());
    let property = |p: &str| Content::Property(p.to_owned());
    let cell = |x0: f64, x1: f64, y0: f64, y1: f64, label: &str, content, height| Cell {
        at: [x0, y0],
        size: [x1 - x0, y1 - y0],
        label: label.to_owned(),
        content,
        height,
    };
    vec![
        cell(
            0.0,
            26.0,
            36.0,
            45.0,
            "Projection",
            property("Projection"),
            2.5,
        ),
        cell(26.0, 48.0, 36.0, 45.0, "Scale", property("Scale"), 3.5),
        cell(
            48.0,
            100.0,
            36.0,
            45.0,
            "General tolerances",
            text("ISO 2768-mK"),
            3.5,
        ),
        cell(
            100.0,
            152.0,
            36.0,
            45.0,
            "Material",
            property("Material"),
            3.5,
        ),
        cell(152.0, 180.0, 36.0, 45.0, "Mass", property("Mass"), 3.5),
        cell(0.0, 48.0, 27.0, 36.0, "Responsible dept.", text(""), 2.5),
        cell(
            48.0,
            100.0,
            27.0,
            36.0,
            "Technical reference",
            text(""),
            2.5,
        ),
        cell(
            100.0,
            152.0,
            27.0,
            36.0,
            "Document type",
            property("Document type"),
            3.5,
        ),
        cell(
            152.0,
            180.0,
            27.0,
            36.0,
            "Document status",
            property("Status"),
            2.5,
        ),
        cell(0.0, 48.0, 0.0, 27.0, "Legal owner", text(""), 3.5),
        cell(
            48.0,
            100.0,
            13.5,
            27.0,
            "Created by",
            property("Author"),
            3.5,
        ),
        cell(
            48.0,
            100.0,
            0.0,
            13.5,
            "Approved by",
            property("Approved by"),
            3.5,
        ),
        cell(100.0, 152.0, 9.0, 27.0, "Title", property("Title"), 5.0),
        cell(
            152.0,
            180.0,
            9.0,
            27.0,
            "Identification number",
            property("Part number"),
            3.5,
        ),
        cell(100.0, 113.0, 0.0, 9.0, "Rev.", property("Revision"), 3.5),
        cell(
            113.0,
            140.0,
            0.0,
            9.0,
            "Date of issue",
            property("Date"),
            3.5,
        ),
        cell(140.0, 152.0, 0.0, 9.0, "Lang.", text("en"), 3.5),
        cell(152.0, 166.0, 0.0, 9.0, "Size", property("Format"), 3.5),
        cell(166.0, 180.0, 0.0, 9.0, "Sheet", property("Sheet"), 3.5),
    ]
}

impl Template {
    /// The width and the height of its sheet, in millimetres.
    pub fn size(&self) -> [f64; 2] {
        let (long, short) = self.format.sides();
        if self.upright {
            [short, long]
        } else {
            [long, short]
        }
    }

    /// A sheet of `format` with the three usual views — from the
    /// front, and from the left and from above beside it — and the
    /// title block of ISO 7200.
    pub fn three_views(name: &str, format: Format, upright: bool) -> Self {
        let mut template = Self {
            name: name.to_owned(),
            format,
            upright,
            projection: Projection::FirstAngle,
            views: Vec::new(),
            block: iso_block(),
            spacing: Default::default(),
            drafting: Default::default(),
        };
        let [w, h] = template.size();
        let view = |id: &str, kind, at| SheetView {
            id: id.to_owned(),
            kind,
            at,
            scale: None,
            style: Style::HiddenRemoved,
            bodies: Vec::new(),
            cut_only: false,
            half: None,
            section: None,
            caption_at: None,
        };
        let front = if upright {
            [w * 0.36, h * 0.72]
        } else {
            [w * 0.32, h * 0.62]
        };
        let beside = |beside| ViewKind::Projected {
            of: "front".to_owned(),
            beside,
        };
        template.views = vec![
            view("front", ViewKind::Base { side: Side::Front }, front),
            view("left", beside(Beside::Right), front),
            view("top", beside(Beside::Below), front),
        ];
        template
    }
}

/// The sheet templates of a workspace.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Templates {
    /// The templates: the first is what a drawing starts from where
    /// nothing else is said.
    pub templates: Vec<Template>,
}

impl Default for Templates {
    fn default() -> Self {
        let mut iso = Template::three_views("ISO A3", Format::A3, false);
        // (And the model as it stands in space, in the corner.)
        let [w, h] = iso.size();
        iso.views.push(SheetView {
            id: "isometric".to_owned(),
            kind: ViewKind::Base {
                side: Side::Isometric,
            },
            at: [w * 0.74, h * 0.74],
            scale: None,
            style: Style::HiddenRemoved,
            bodies: Vec::new(),
            cut_only: false,
            half: None,
            section: None,
            caption_at: None,
        });
        Self {
            templates: vec![
                Template::three_views("ISO A3, three views", Format::A3, false),
                iso_named(iso, "ISO A3, with isometric"),
                Template::three_views("ISO A4 upright", Format::A4, true),
            ],
        }
    }
}

fn iso_named(mut template: Template, name: &str) -> Template {
    template.name = name.to_owned();
    template
}

impl Templates {
    /// The template called `name`.
    pub fn get(&self, name: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.name == name)
    }

    /// Their names.
    pub fn names(&self) -> Vec<String> {
        self.templates.iter().map(|t| t.name.clone()).collect()
    }

    /// A name no template has yet.
    pub fn fresh_name(&self) -> String {
        (1..)
            .map(|n| match n {
                1 => "Template".to_owned(),
                n => format!("Template {n}"),
            })
            .find(|name| self.get(name).is_none())
            .expect("unbounded range")
    }

    /// The templates as the text of their file.
    pub fn to_text(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }

    /// The templates the text of a file holds.
    pub fn from_text(text: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
}
