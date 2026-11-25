use shapecraft_core::Hotkey;
use shapecraft_core::widgets::Button;

use super::Icon;
use crate::{Message, Model, Tab};

/// One button of the ribbon. Its button has a hotkey.
pub struct Tool {
    /// What it shows.
    pub icon: Icon,
    /// What it does, in a sentence.
    pub hint: &'static str,
    /// Whether it does anything now.
    pub enabled: bool,
    /// The button: its label and message.
    pub button: Button<Message>,
    /// The group of tools it stands with (those that construct what is
    /// built on, those that create bodies, those that change them); the
    /// empty group for the tools that stand alone.
    pub group: &'static str,
    /// Whether it is shown small, stacked with the others of its group.
    pub small: bool,
}

/// The tools with their groups, those of a group together: what is
/// built on first, then what creates bodies, what bends and changes
/// them, and what arranges them; the tools that stand alone stay where
/// they are, after those.
fn grouped(mut tools: Vec<Tool>) -> Vec<Tool> {
    const ORDER: [&str; 17] = [
        "Part",
        "Look",
        "Picture",
        "Sketch",
        "Construct",
        "Create",
        "Bend",
        "Modify",
        "Bodies",
        "Direct",
        "Define",
        "Drawing",
        "Views",
        "Surfaces",
        "Edit",
        "Sew",
        "",
    ];
    // In a group, the tools used most come first, large; the others
    // follow small, stacked.
    tools.sort_by_key(|tool| (ORDER.iter().position(|g| *g == tool.group), tool.small));
    tools
}

/// A tab of the ribbon, to switch to.
pub struct TabButton {
    /// The tab.
    pub tab: Tab,
    /// Whether it is the one that shows.
    pub active: bool,
    /// Switches to it.
    pub button: Button<Message>,
}

/// The tool bar: undo and redo, the tabs, then the tools of the tab that
/// shows, one per operation, for the focused part. Adding an operation
/// opens it for editing.
pub struct Ribbon {
    /// The tabs.
    pub tabs: Vec<TabButton>,
    /// Undo and redo, then the tools of the tab that shows, in order.
    pub tools: Vec<Tool>,
}

impl Ribbon {
    /// The tools for `model`: undo and redo, and one per operation. They
    /// add to the focused part.
    pub fn new(model: &Model) -> Self {
        let tool = |icon, enabled, label: &str, hotkey: Hotkey, hint, message: Message| Tool {
            icon,
            hint,
            enabled,
            button: Button::new(label, move || message.clone()).with_hotkey(hotkey),
            group: "",
            small: false,
        };
        let mut tools = vec![
            tool(
                Icon::Undo,
                model.can_undo(),
                "Undo",
                Hotkey::ctrl('Z'),
                "Undo the last change",
                Message::Undo,
            ),
            tool(
                Icon::Redo,
                model.can_redo(),
                "Redo",
                Hotkey::ctrl_shift('Z'),
                "Redo what was undone",
                Message::Redo,
            ),
        ];
        let tabs = Tab::ALL
            .into_iter()
            .zip(['1', '2', '3', '4', '5', '6', '7'])
            .map(|(tab, key)| TabButton {
                tab,
                active: model.tab == tab,
                button: Button::new(tab.label(), move || Message::SetTab(tab))
                    .with_hotkey(Hotkey::ctrl(key)),
            })
            .collect();
        let Some(path) = model.focused_part() else {
            return Self { tabs, tools };
        };
        let Ok(part) = model.workspace.part(&path) else {
            return Self { tabs, tools };
        };
        // The tools of the tab that shows: those the crates of
        // operations told of for it, each making its operation from what
        // is selected.
        let world = &model.viewport.shown.world;
        let mut selection = model.selected_sources();
        // The step selected in the timeline counts as what it made: a
        // sketch, a datum, a body — a tool started then takes it.
        if let Some([id]) = model.step.as_deref() {
            let made = if world.profiles.contains_key(id) {
                Some(shapecraft_core::Source::Profile(id.clone()))
            } else if world.datums.contains_key(id) {
                Some(shapecraft_core::Source::Datum(id.clone()))
            } else {
                world
                    .topo
                    .body_called(id)
                    .map(|_| shapecraft_core::Source::Topology(id.clone()))
            };
            selection.extend(made.filter(|source| !selection.contains(source)));
        }
        let fresh_id = |stem: &str| part.fresh_id(stem);
        let context = shapecraft_core::ToolContext {
            fresh_id: &fresh_id,
            world,
            unit: part.unit,
            selection: &selection,
        };
        let mut of_tab = Vec::new();
        for told in shapecraft_cad_part::registry().tools() {
            if told.tab != model.tab.label() {
                continue;
            }
            let (op, _) =
                crate::edit::with_selection(model, &path, (told.make)(&context), &selection);
            // (What a drawing starts from is asked first.)
            let message = match told.label {
                "Drawing" => Message::ShowPanel(Some(crate::Panel::NewDrawing)),
                _ => Message::AddOperation {
                    part: path.clone(),
                    op,
                },
            };
            // (A small letter stands for the key with shift.)
            let hotkey = if told.key.is_ascii_lowercase() {
                Hotkey::shift(told.key.to_ascii_uppercase())
            } else {
                Hotkey::key(told.key)
            };
            let mut made = tool(
                Icon::Named(told.icon),
                true,
                told.label,
                hotkey,
                told.hint,
                message,
            );
            made.group = told.group;
            made.small = told.small;
            of_tab.push(made);
            // A new part is made beside where one is added.
            if told.label == "Add part" {
                of_tab.push(tool(
                    Icon::NewPart,
                    true,
                    "New part",
                    Hotkey::key('N'),
                    "Make a new part, add it here, and work on it in place",
                    Message::NewPartHere(None),
                ));
            }
        }
        // What the part is made of is no step: the materials and the
        // parameters show in panels.
        if model.tab == Tab::Properties {
            let panel = |icon, label: &str, key, hint, panel| {
                let mut made = tool(
                    Icon::Named(icon),
                    true,
                    label,
                    Hotkey::key(key),
                    hint,
                    Message::ShowPanel(Some(panel)),
                );
                made.group = "Part";
                made
            };
            of_tab.push(panel(
                "materials",
                "Materials",
                'M',
                "What the part is made of, and the materials there are: their densities, colours and textures, and how they take light",
                crate::Panel::Materials,
            ));
            of_tab.push(panel(
                "parameters",
                "Parameters",
                'P',
                "The part's parameters — named numbers its steps' formulas use — and its variants",
                crate::Panel::Parameters,
            ));
        }
        if model.tab == Tab::Properties {
            let mut made = tool(
                Icon::Named("properties"),
                true,
                "Properties",
                Hotkey::key('I'),
                "What the part says of itself — its number, its revision, who made it — and which properties parts have",
                Message::ShowPanel(Some(crate::Panel::Properties)),
            );
            made.group = "Part";
            of_tab.push(made);
        }
        if model.tab == Tab::Annotate {
            let mut made = tool(
                Icon::Named("templates"),
                true,
                "Templates",
                Hotkey::shift('T'),
                "The sheet templates new drawings start from: the sheet's size, its views, and how its title block looks",
                Message::ShowPanel(Some(crate::Panel::Templates)),
            );
            made.group = "Drawing";
            made.small = true;
            of_tab.push(made);
        }
        tools.extend(grouped(of_tab));
        Self { tabs, tools }
    }
}
