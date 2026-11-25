//! Modelling operations.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::builder::{Builder, Cache};
use crate::handles::Handle;
use crate::measure::Parameters;
use crate::reference::Source;
use crate::widgets::{MapFn, ToolButton, Widget};
use crate::world::{DatumPlane, World};

/// Where an operation's dialog is shown while it is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    /// In a popup beside the viewport: a few fields.
    #[default]
    Popup,
    /// In the ribbon at the top, in place of the tools: an operation that
    /// is a workbench of its own, like a sketch.
    Ribbon,
}

/// What an operation is edited against: the world it will be applied to,
/// and what earlier builds of the part worked out.
#[derive(Clone, Copy)]
pub struct EditContext<'a> {
    /// The world the operation will be applied to. Shared, so handlers
    /// that need it later (to project an edge that is picked) can keep it.
    pub world: &'a Arc<World>,
    /// The part's cache of solved values (see [`Cache`]).
    pub cache: &'a Cache,
    /// The part's parameters, as it is by default: what formulas entered
    /// in the dialog are valued with.
    pub parameters: &'a Parameters,
    /// The parts that can be placed.
    pub parts: &'a dyn Parts,
    /// The name of the step being edited (what it makes is called
    /// after it); empty where no step is.
    pub step: &'a str,
}

/// What an operation can know about the other parts while it is edited.
pub trait Parts {
    /// The names of the variants of the part at `path`.
    fn variants(&self, path: &str) -> Vec<String>;

    /// The names of the materials there are to choose from.
    fn materials(&self) -> Vec<String> {
        Vec::new()
    }

    /// The names of the bodies of `variant` of the part at `path` (the
    /// part as it is by default, for `None`), as far as they are known
    /// without building it.
    fn bodies(&self, path: &str, variant: Option<&str>) -> Vec<String> {
        let _ = (path, variant);
        Vec::new()
    }
}

/// No parts: for editing outside a workspace.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoParts;

impl Parts for NoParts {
    fn variants(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

impl<'a> EditContext<'a> {
    /// A context outside a workspace: no parameters, no other parts.
    pub fn standalone(world: &'a Arc<World>, cache: &'a Cache) -> Self {
        Self {
            world,
            cache,
            parameters: &crate::measure::NO_PARAMETERS,
            parts: &NoParts,
            step: "",
        }
    }
}

/// What the step has in its part's [`Cache`]: its entries, by their names
/// after the step's id.
pub type Kept = Vec<(String, Vec<f64>)>;

/// What an edit changes: the operation's arguments, the state of the edit
/// session, what the operation keeps in its part's cache, or several of
/// them.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit<A, S> {
    /// The new arguments, if they change. Recorded for undo, and saved.
    pub args: Option<A>,
    /// The new session state, if it changes.
    pub state: Option<S>,
    /// What the step keeps in the cache from now on, if that changes:
    /// all its entries. For what a step works out and the user may also
    /// set by hand (where a placed part is). Recorded for undo, and
    /// saved, like the arguments.
    pub kept: Option<Kept>,
}

impl<A, S> Edit<A, S> {
    /// New arguments.
    pub fn args(args: A) -> Self {
        Self {
            args: Some(args),
            state: None,
            kept: None,
        }
    }

    /// A new session state.
    pub fn state(state: S) -> Self {
        Self {
            args: None,
            state: Some(state),
            kept: None,
        }
    }

    /// What the step keeps in the cache from now on.
    pub fn kept(kept: Kept) -> Self {
        Self {
            args: None,
            state: None,
            kept: Some(kept),
        }
    }

    /// New arguments and a new session state.
    pub fn both(args: A, state: S) -> Self {
        Self {
            args: Some(args),
            state: Some(state),
            kept: None,
        }
    }
}

/// What an operation shows while it is edited: a dialog and named 3D
/// handles in the viewport, producing messages of type `M` (see
/// [`crate::widgets`]). An operation's visualization produces its new
/// arguments.
pub struct EditVisualization<M> {
    /// The dialog's root widget.
    pub dialog: Widget<M>,
    /// Where the dialog is shown.
    pub placement: Placement,
    /// For a dialog in the ribbon: the fields of what is selected in
    /// the step (a sketch's dimension, its curves), shown in a panel at
    /// the side as other steps' dialogs are; `None` while nothing is.
    pub inspector: Option<Widget<M>>,
    /// What the context menu (the right or middle button, held) offers for what is selected
    /// in the operation, e.g. the constraints a sketch's selection takes.
    pub menu: Vec<ToolButton<M>>,
    /// Viewport handles, by name (ordered, so drawing is deterministic).
    pub handles: BTreeMap<String, Handle<M>>,
    /// What a click on nothing in the scene does, by the ray of the
    /// pointer, for a step that is placed that way (a dimension's text
    /// stands where it is clicked): the step is done with that, and the
    /// next of its kind starts — as a sketch's dimension tool goes on.
    pub place: Option<Placer<M>>,
}

/// What a click on nothing does, by the ray of the pointer (see
/// [`EditVisualization::place`]); nothing while the step is not ready
/// for it.
pub type Placer<M> = Rc<dyn Fn(&crate::ray::Ray) -> Option<M>>;

impl<M: 'static> EditVisualization<M> {
    /// A visualization with `dialog` and the viewport `handles`, by name.
    pub fn new(dialog: impl Into<Widget<M>>, handles: BTreeMap<String, Handle<M>>) -> Self {
        Self {
            dialog: dialog.into(),
            placement: Placement::Popup,
            inspector: None,
            menu: Vec::new(),
            handles,
            place: None,
        }
    }

    /// The same visualization, of a step that a click on nothing in
    /// the scene places (see [`Self::place`]).
    #[must_use]
    pub fn placed_by(mut self, place: impl Fn(&crate::ray::Ray) -> Option<M> + 'static) -> Self {
        self.place = Some(Rc::new(place));
        self
    }

    /// The same visualization, its dialog shown in the ribbon.
    #[must_use]
    pub fn in_ribbon(mut self) -> Self {
        self.placement = Placement::Ribbon;
        self
    }

    /// The same visualization, with `inspector` in the panel at the side
    /// (see [`Self::inspector`]).
    #[must_use]
    pub fn with_inspector(mut self, inspector: Option<Widget<M>>) -> Self {
        self.inspector = inspector;
        self
    }

    /// The same visualization, offering `menu` in the context menu.
    #[must_use]
    pub fn with_menu(mut self, menu: Vec<ToolButton<M>>) -> Self {
        self.menu = menu;
        self
    }

    /// The same visualization, its messages converted by `f`.
    pub fn map<N: 'static>(self, f: impl Fn(M) -> N + 'static) -> EditVisualization<N> {
        let f: MapFn<M, N> = Rc::new(f);
        EditVisualization {
            dialog: self.dialog.map_rc(&f),
            placement: self.placement,
            inspector: self.inspector.map(|w| w.map_rc(&f)),
            menu: self.menu.into_iter().map(|t| t.map_rc(&f)).collect(),
            handles: self
                .handles
                .into_iter()
                .map(|(name, h)| (name, h.map_rc(&f)))
                .collect(),
            place: self.place.map(|place| {
                let f = f.clone();
                Rc::new(move |ray: &crate::ray::Ray| place(ray).map(|m| f(m))) as Placer<N>
            }),
        }
    }
}

/// A body an operation made, as what writes it: the builder has it
/// written into the world, under the name it gets.
pub trait Shape: Send + Sync {
    /// Writes the body into `topo`, called `name`.
    fn write(&self, topo: &mut crate::world::topo::Topo, name: &str) -> anyhow::Result<()>;
}

impl<F> Shape for F
where
    F: Fn(&mut crate::world::topo::Topo, &str) -> anyhow::Result<()> + Send + Sync,
{
    fn write(&self, topo: &mut crate::world::topo::Topo, name: &str) -> anyhow::Result<()> {
        self(topo, name)
    }
}

/// What an operation made, for the builder to put in the world.
#[derive(Default)]
pub struct Made<'a> {
    /// Datums, by the names they get.
    pub datums: Vec<(String, crate::world::Datum)>,
    /// The bodies that are no more: those it changed are among them,
    /// and made again.
    pub removed: Vec<String>,
    /// Bodies, by the names they get, in the order they are made.
    pub bodies: Vec<(String, Box<dyn Shape + 'a>)>,
    /// Sketches, by the names they get.
    pub profiles: Vec<(String, crate::world::Profile)>,
    /// Joints between placed parts.
    pub joints: Vec<crate::world::Joint>,
    /// Couplings of joints.
    pub couplings: Vec<crate::world::Coupling>,
    /// What steps keep in the part's cache from now on: by step, all
    /// its entries.
    pub kept: Vec<(String, Kept)>,
    /// Placed parts, by name: as they are from now on, or (none) gone.
    pub instances: Vec<(String, Option<crate::world::Instance>)>,
    /// Joints there were, as they are from now on, each in place of
    /// the one of its name (one held where it is driven to).
    pub joints_changed: Vec<crate::world::Joint>,
    /// What is said of the model, by the names it gets.
    pub annotations: Vec<(String, crate::world::Annotation)>,
    /// How bodies and faces look from now on, by their names: each over
    /// what was said of it before.
    pub looks: Vec<(String, crate::world::Look)>,
    /// Threads said of faces, not cut into them, by the names they
    /// get.
    pub threads: Vec<(String, crate::world::Thread)>,
    /// Named views of the model, by the names they get.
    pub views: Vec<(String, crate::world::Viewpoint)>,
    /// Where the model is cut open, by the names the cuts get.
    pub sections: Vec<(String, crate::world::Cutting)>,
    /// Mass properties asked for, by the names they get.
    pub masses: Vec<(String, crate::world::MassProperties)>,
}

impl<'a> Made<'a> {
    /// The one datum `datum`, to be called `name`.
    pub fn datum(name: impl Into<String>, datum: impl Into<crate::world::Datum>) -> Self {
        Self {
            datums: vec![(name.into(), datum.into())],
            ..Self::default()
        }
    }

    /// The one body `shape`, to be called `name`.
    pub fn body(name: impl Into<String>, shape: impl Shape + 'a) -> Self {
        Self::default().and(name, shape)
    }

    /// The same, and the body `shape`, to be called `name`.
    #[must_use]
    pub fn and(mut self, name: impl Into<String>, shape: impl Shape + 'a) -> Self {
        self.bodies.push((name.into(), Box::new(shape)));
        self
    }
}

/// One applied modelling operation: an id, typed arguments, and how they
/// change the world.
///
/// Arguments are what the user asked for: they are saved, and every change
/// to them can be undone. They refer to existing geometry through
/// [`crate::Reference`]s and resolve them with [`World::resolve`].
///
/// Two more kinds of state keep the arguments clean:
///
/// * What an operation *works out* from its arguments (a sketch's solved
///   points) goes into the part's [`Cache`] when it is applied, and is the
///   starting point the next time.
/// * What only matters *while it is edited* (the tool in use, what is
///   selected inside it) is its [`EditState`](Operation::EditState).
///
/// While the user edits an operation, it shows an [`EditVisualization`]
/// whose messages are [`Edit`]s: new arguments, which the owner of the
/// operation stores with [`Operation::args_mut`], a new edit state, or
/// both.
pub trait Operation {
    /// The operation's arguments.
    type Args: crate::Arguments;

    /// The state of an edit session that is not part of the arguments:
    /// the tool in use, a half-drawn line, what is selected inside the
    /// operation. It lives while the operation is edited; it is neither
    /// saved nor undone. `()` for operations that have none.
    type EditState: Clone + Default + PartialEq + std::fmt::Debug + 'static;

    /// The operation's id, unique within its part. Operations that create
    /// a datum name it after their id.
    fn id(&self) -> &str;

    /// The id, for renaming the operation. What it makes is named after
    /// it, so the operations that refer to that have to follow (see
    /// [`Source::rename`]).
    fn id_mut(&mut self) -> &mut String;

    /// The arguments.
    fn args(&self) -> &Self::Args;

    /// The arguments, for replacing with an edit's new ones.
    fn args_mut(&mut self) -> &mut Self::Args;

    /// Applies the operation to the world being built. What it reads
    /// of the world it reads through `builder`, which notes it: with
    /// the same of it the operation does the same, and a builder need
    /// not apply it again. Operations that place other parts build
    /// them through `builder`.
    ///
    /// An operation that only makes things ([`make`](Self::make)) has
    /// no more to apply than putting them in the world.
    fn apply(&self, builder: &mut dyn Builder) -> anyhow::Result<()> {
        let made = self.make(builder)?;
        builder.put(made)
    }

    /// What the operation makes, of the world as `builder` lets it read
    /// it: a function of its arguments and of what it reads, changing
    /// nothing. For the operations that are that and no more; the
    /// others change the world themselves, in [`apply`](Self::apply).
    fn make(&self, builder: &dyn Builder) -> anyhow::Result<Made<'_>> {
        let _ = builder;
        anyhow::bail!("operation {:?} changes the world itself", self.id())
    }

    /// What the arguments refer to: the sources of their references. A
    /// datum referred to is *used*, so the UI hides it by default. What
    /// the arguments say they refer to, unless the operation knows
    /// better.
    fn references(&self) -> Vec<Source> {
        crate::Arguments::references(self.args())
    }

    /// The same sources, for pointing them elsewhere: after a rename of
    /// the operation that makes what they point at.
    fn references_mut(&mut self) -> Vec<&mut Source> {
        crate::Arguments::references_mut(self.args_mut())
    }

    /// What the operation does, in a few words, for a list of steps
    /// ("3 along z, 2 apart"). Nothing, unless it says.
    fn summary(&self) -> String {
        String::new()
    }

    /// The dialog and handles that edit the operation, for an edit session
    /// in `state`, placed against `context` (the world the operation will
    /// be applied to). Their messages change the arguments, the session
    /// state, or both. The fields of the arguments, one below the
    /// other, unless the operation lays out its own.
    fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        state: &Self::EditState,
    ) -> EditVisualization<Edit<Self::Args, Self::EditState>> {
        let _ = state;
        let fields = crate::Arguments::fields(self.args(), context);
        EditVisualization::new(crate::widgets::VBox::new(fields), BTreeMap::new()).map(Edit::args)
    }

    /// The plane the operation is edited in, if it has one: the view turns
    /// to face it when the edit starts.
    fn edit_plane(&self, context: &EditContext<'_>) -> Option<DatumPlane> {
        let _ = context;
        None
    }

    /// The part it places in this one, if it places one: its path, and
    /// the variant it is placed as. What is built depends on that part
    /// as it is.
    fn placed(&self) -> Option<(&str, Option<&str>)> {
        None
    }

    /// The files it reads, besides parts: what is built depends on
    /// them as they are on disk.
    fn files(&self) -> Vec<&str> {
        Vec::new()
    }

    /// Whether what it keeps in the part's cache is settled only after
    /// the part's last step — where the parts of an assembly end up —
    /// so that no step starts from it.
    fn settles(&self) -> bool {
        false
    }
}

/// The dialog and handles that edit `op`'s arguments against `world`,
/// outside an edit session (no cache, the default edit state): their
/// messages are the edited arguments. For tests and tools that look at an
/// operation's fields; messages that change only the edit state panic.
pub fn edit_arguments<O: Operation>(op: &O, world: &World) -> EditVisualization<O::Args> {
    let world = Arc::new(world.clone());
    let cache = Cache::default();
    let context = EditContext::standalone(&world, &cache);
    op.visualize_edit(&context, &O::EditState::default())
        .map(|edit| edit.args.expect("an edit of the arguments"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::Standalone;
    use crate::widgets::{Button, VBox};
    use crate::world::{Datum, DatumPlane};

    /// A minimal operation: store a datum under its id.
    struct AddDatum {
        id: String,
        args: Datum,
    }

    impl crate::Arguments for Datum {
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

    impl Operation for AddDatum {
        type Args = Datum;
        type EditState = ();

        fn id(&self) -> &str {
            &self.id
        }

        fn id_mut(&mut self) -> &mut String {
            &mut self.id
        }

        fn args(&self) -> &Datum {
            &self.args
        }

        fn args_mut(&mut self) -> &mut Datum {
            &mut self.args
        }

        fn apply(&self, builder: &mut dyn Builder) -> anyhow::Result<()> {
            let world = builder.world_mut();
            anyhow::ensure!(!world.datums.contains_key(&self.id), "datum exists");
            world.datums.insert(self.id.clone(), self.args);
            Ok(())
        }

        fn visualize_edit(
            &self,
            _: &EditContext<'_>,
            (): &(),
        ) -> EditVisualization<Edit<Datum, ()>> {
            EditVisualization::new(
                VBox::new(vec![
                    Button::new("xy", || Edit::args(DatumPlane::xy().into())).into(),
                ]),
                BTreeMap::new(),
            )
        }
    }

    #[test]
    fn operations_change_the_world_and_edit_their_args() -> anyhow::Result<()> {
        let mut builder = Standalone::new();
        let mut op = AddDatum {
            id: "top".into(),
            args: DatumPlane::yz().into(),
        };
        op.apply(&mut builder)?;
        assert!(builder.world().datums.contains_key("top"));
        assert!(op.apply(&mut builder).is_err());

        let world = Arc::new(builder.world().clone());
        let cache = Cache::default();
        let context = EditContext::standalone(&world, &cache);
        let edit = op.visualize_edit(&context, &());
        assert!(edit.handles.is_empty());
        let Widget::VBox(vbox) = edit.dialog else {
            anyhow::bail!("expected a vbox");
        };
        let Some(Widget::Button(button)) = vbox.children.first() else {
            anyhow::bail!("expected a button");
        };
        *op.args_mut() = button.click().args.expect("new arguments");
        assert_eq!(op.args, Datum::Plane(DatumPlane::xy()));
        Ok(())
    }
}
