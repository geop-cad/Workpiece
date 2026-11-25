//! Drawing: the step that is a drawing of the model as it is where the
//! step stands in the part's program.

use shapecraft_core::{Builder, Edit, EditContext, EditVisualization, Operation, Source, Widget};

use crate::Sheet;

/// A drawing of the model: its sheet. The model is drawn as the steps
/// before this one leave it; nothing of the model follows from it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Drawing {
    /// The operation's id: what the drawing is called.
    pub id: String,
    /// The sheet.
    pub args: Sheet,
}

/// A sheet refers to what is said of the model by name, and is edited
/// on the sheet itself: it has no fields.
impl shapecraft_core::Arguments for Sheet {
    fn references(&self) -> Vec<Source> {
        Vec::new()
    }

    fn references_mut(&mut self) -> Vec<&mut Source> {
        Vec::new()
    }

    fn fields(&self, _: &EditContext<'_>) -> Vec<Widget<Self>> {
        Vec::new()
    }
}

impl Operation for Drawing {
    type Args = Sheet;
    type EditState = shapecraft_ops_sketch::SketchState;

    fn id(&self) -> &str {
        &self.id
    }

    fn id_mut(&mut self) -> &mut String {
        &mut self.id
    }

    fn args(&self) -> &Sheet {
        &self.args
    }

    fn args_mut(&mut self) -> &mut Sheet {
        &mut self.args
    }

    fn make(&self, _: &dyn Builder) -> anyhow::Result<shapecraft_core::Made<'_>> {
        // Every view but a base view is of another, which has to be
        // there.
        for view in &self.args.views {
            use crate::sheet::ViewKind;
            let of = match &view.kind {
                ViewKind::Base { .. } | ViewKind::Named { .. } => continue,
                ViewKind::Section { of, .. } => match of {
                    Some(of) => of,
                    None => continue,
                },
                ViewKind::Projected { of, .. }
                | ViewKind::Auxiliary { of, .. }
                | ViewKind::Detail { of, .. } => of,
            };
            anyhow::ensure!(
                self.args.view(of).is_some(),
                "the view {:?} is of {of:?}, which the sheet has not",
                view.id
            );
        }
        Ok(shapecraft_core::Made::default())
    }

    fn summary(&self) -> String {
        let sheet = &self.args;
        let views = sheet.views.len();
        format!(
            "{}, {}, {views} view{}",
            sheet.format.label(),
            crate::lay::scale_text(sheet.scale),
            if views == 1 { "" } else { "s" }
        )
    }

    /// The sheet is its own workbench: it shows in place of the model,
    /// and is edited there. What it gives here is what draws on the
    /// sheet by hand: the tools of a sketch, and the sketch's canvas
    /// lying on the sheet.
    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        state: &shapecraft_ops_sketch::SketchState,
    ) -> EditVisualization<Edit<Sheet, shapecraft_ops_sketch::SketchState>> {
        let sketch = self.args.hand();
        let sheet = self.args.clone();
        // (Nothing of the part's cache is the sheet's.)
        let none = shapecraft_core::Cache::default();
        let context = EditContext {
            world: context.world,
            cache: &none,
            parameters: context.parameters,
            parts: context.parts,
            step: "",
        };
        sketch
            .edit_on(
                &context,
                state,
                Some(shapecraft_core::world::DatumPlane::xy()),
            )
            .map(move |edit| Edit {
                args: edit.args.map(|drawn| Sheet {
                    drawn: Some(drawn),
                    ..sheet.clone()
                }),
                state: edit.state,
                kept: None,
            })
    }
}
