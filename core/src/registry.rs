//! The operations there are, told by the crates that have them.
//!
//! A crate of operations has a `register` function that tells a
//! [`Registry`] of each of its operations: its kind (what its steps are
//! called in a part's file), the tools that add it (with their label,
//! icon, key and where in the ribbon they stand) and the icons it
//! draws. Nothing else lists the operations: a part's program holds
//! them as [`AnyOperation`], which is any of them.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::OnceLock;

use crate::builder::Builder;
use crate::operation::{EditContext, EditVisualization, Kept, Operation};
use crate::reference::Source;
use crate::world::{DatumPlane, World};

/// An operation of any kind: what [`AnyOperation`] holds. Every
/// [`Operation`] that can be copied, compared and written is one.
pub trait ErasedOperation: Any + Debug + Send + Sync {
    /// What its kind is called: its type's name, which is what a part's
    /// file calls its steps.
    fn kind(&self) -> &'static str;
    /// A copy of it.
    fn boxed(&self) -> Box<dyn ErasedOperation>;
    /// Whether `other` is of its kind and the same.
    fn same(&self, other: &dyn ErasedOperation) -> bool;
    /// It as a value to write.
    fn value(&self) -> Result<serde_json::Value, serde_json::Error>;
    /// It, to be asked what kind it is.
    fn as_any(&self) -> &dyn Any;
    /// The same, to change it.
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// See [`Operation::id`].
    fn erased_id(&self) -> &str;
    /// See [`Operation::id_mut`].
    fn erased_id_mut(&mut self) -> &mut String;
    /// See [`Operation::apply`].
    fn erased_apply(&self, builder: &mut dyn Builder) -> anyhow::Result<()>;
    /// See [`Operation::references`].
    fn erased_references(&self) -> Vec<Source>;
    /// See [`Operation::references_mut`].
    fn erased_references_mut(&mut self) -> Vec<&mut Source>;
    /// See [`Operation::summary`].
    fn erased_summary(&self) -> String;
    /// See [`Operation::edit_plane`].
    fn erased_edit_plane(&self, context: &EditContext<'_>) -> Option<DatumPlane>;
    /// See [`Operation::placed`].
    fn erased_placed(&self) -> Option<(&str, Option<&str>)>;
    /// See [`Operation::files`].
    fn erased_files(&self) -> Vec<&str>;
    /// See [`Operation::settles`].
    fn erased_settles(&self) -> bool;
    /// See [`Operation::visualize_edit`]: in `state` if that is a state
    /// of this kind of operation, else in its default one.
    fn erased_visualize_edit(
        &self,
        context: &EditContext<'_>,
        state: Option<&AnyEditState>,
    ) -> EditVisualization<AnyEdit>;
}

/// The last part of a type's name: `Extrude` of `a::b::Extrude`.
fn name_of<T>() -> &'static str {
    let whole = std::any::type_name::<T>();
    whole.rsplit("::").next().unwrap_or(whole)
}

impl<T> ErasedOperation for T
where
    T: Operation + Clone + PartialEq + Debug + serde::Serialize + Send + Sync + 'static,
    T::EditState: Send + Sync,
{
    fn kind(&self) -> &'static str {
        name_of::<T>()
    }

    fn boxed(&self) -> Box<dyn ErasedOperation> {
        Box::new(self.clone())
    }

    fn same(&self, other: &dyn ErasedOperation) -> bool {
        other.as_any().downcast_ref::<T>() == Some(self)
    }

    fn value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn erased_id(&self) -> &str {
        self.id()
    }

    fn erased_id_mut(&mut self) -> &mut String {
        self.id_mut()
    }

    fn erased_apply(&self, builder: &mut dyn Builder) -> anyhow::Result<()> {
        self.apply(builder)
    }

    fn erased_references(&self) -> Vec<Source> {
        self.references()
    }

    fn erased_references_mut(&mut self) -> Vec<&mut Source> {
        self.references_mut()
    }

    fn erased_summary(&self) -> String {
        self.summary()
    }

    fn erased_edit_plane(&self, context: &EditContext<'_>) -> Option<DatumPlane> {
        self.edit_plane(context)
    }

    fn erased_placed(&self) -> Option<(&str, Option<&str>)> {
        self.placed()
    }

    fn erased_files(&self) -> Vec<&str> {
        self.files()
    }

    fn erased_settles(&self) -> bool {
        self.settles()
    }

    fn erased_visualize_edit(
        &self,
        context: &EditContext<'_>,
        state: Option<&AnyEditState>,
    ) -> EditVisualization<AnyEdit> {
        let fresh = T::EditState::default();
        let state = state
            .and_then(|s| s.downcast_ref::<T::EditState>())
            .unwrap_or(&fresh);
        let original = self.clone();
        // (The step edited is this one.)
        let context = &EditContext {
            step: self.id(),
            ..*context
        };
        self.visualize_edit(context, state)
            .map(move |edit| AnyEdit {
                op: edit.args.map(|args| {
                    let mut edited = original.clone();
                    *edited.args_mut() = args;
                    AnyOperation::new(edited)
                }),
                state: edit.state.map(AnyEditState::new),
                kept: edit.kept,
            })
    }
}

/// Any operation a part's program can hold.
#[derive(Debug)]
pub struct AnyOperation(Box<dyn ErasedOperation>);

impl Clone for AnyOperation {
    fn clone(&self) -> Self {
        Self(self.0.boxed())
    }
}

impl PartialEq for AnyOperation {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(&*other.0)
    }
}

impl<T> From<T> for AnyOperation
where
    T: Operation + Clone + PartialEq + Debug + serde::Serialize + Send + Sync + 'static,
    T::EditState: Send + Sync,
{
    fn from(op: T) -> Self {
        Self::new(op)
    }
}

impl AnyOperation {
    /// `op`, as any operation.
    pub fn new(op: impl ErasedOperation) -> Self {
        Self(Box::new(op))
    }

    /// What its kind is called: `Extrude`.
    pub fn kind(&self) -> &'static str {
        self.0.kind()
    }

    /// Whether it is a `T`.
    pub fn is<T: Operation + 'static>(&self) -> bool {
        self.0.as_any().is::<T>()
    }

    /// It as a `T`, if it is one.
    pub fn downcast_ref<T: Operation + 'static>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }

    /// It as a `T` to change, if it is one.
    pub fn downcast_mut<T: Operation + 'static>(&mut self) -> Option<&mut T> {
        self.0.as_any_mut().downcast_mut()
    }

    /// The operation's id.
    pub fn id(&self) -> &str {
        self.0.erased_id()
    }

    /// Applies the operation to the world being built.
    pub fn apply(&self, builder: &mut dyn Builder) -> anyhow::Result<()> {
        self.0.erased_apply(builder)
    }

    /// What the operation refers to.
    pub fn references(&self) -> Vec<Source> {
        self.0.erased_references()
    }

    /// What the operation does, in a few words.
    pub fn summary(&self) -> String {
        self.0.erased_summary()
    }

    /// Renames the operation `id`.
    pub fn set_id(&mut self, id: impl Into<String>) {
        *self.0.erased_id_mut() = id.into();
    }

    /// Follows the operation `from` being renamed `to`: what referred
    /// to something `from` makes refers to what `to` makes.
    pub fn follow_rename(&mut self, from: &str, to: &str) {
        for source in self.0.erased_references_mut() {
            source.rename(from, to);
        }
    }

    /// Points what refers to `from` at `to` instead.
    pub fn retarget(&mut self, from: &Source, to: &Source) {
        for source in self.0.erased_references_mut() {
            if source == from {
                source.clone_from(to);
            }
        }
    }

    /// The dialog and handles that edit the operation in an edit
    /// session in `state` (the default state if that is of another
    /// operation, or none). Their messages are what changes: the edited
    /// operation, to replace this one, the session's new state, or
    /// both.
    pub fn visualize_edit(
        &self,
        context: &EditContext<'_>,
        state: Option<&AnyEditState>,
    ) -> EditVisualization<AnyEdit> {
        self.0.erased_visualize_edit(context, state)
    }

    /// The plane the operation is edited in, if it has one.
    pub fn edit_plane(&self, context: &EditContext<'_>) -> Option<DatumPlane> {
        self.0.erased_edit_plane(context)
    }

    /// The part it places, if it places one: its path and variant.
    pub fn placed(&self) -> Option<(&str, Option<&str>)> {
        self.0.erased_placed()
    }

    /// The files it reads, besides parts.
    pub fn files(&self) -> Vec<&str> {
        self.0.erased_files()
    }

    /// Whether what it keeps in the part's cache is settled only after
    /// the part's last step.
    pub fn settles(&self) -> bool {
        self.0.erased_settles()
    }

    /// The icon it shows as: what its kind was registered with.
    pub fn icon(&self) -> &'static str {
        let kind = installed().and_then(|r| r.kinds.get(self.kind()));
        match kind.map(|k| &k.icon) {
            Some(IconOf::Named(name)) => name,
            Some(IconOf::By(told)) => told(self),
            None => "",
        }
    }
}

impl serde::Serialize for AnyOperation {
    /// As a part's file has its steps: `{"Extrude": {…}}`.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeMap};
        let value = self.0.value().map_err(S::Error::custom)?;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(self.kind(), &value)?;
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for AnyOperation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let step = BTreeMap::<String, serde_json::Value>::deserialize(deserializer)?;
        let mut entries = step.into_iter();
        let (Some((kind, value)), None) = (entries.next(), entries.next()) else {
            return Err(D::Error::custom("a step is one operation"));
        };
        let registry = installed()
            .ok_or_else(|| D::Error::custom("no operations are registered to read a step by"))?;
        let known = registry
            .kinds
            .get(kind.as_str())
            .ok_or_else(|| D::Error::custom(format!("there is no operation {kind:?}")))?;
        (known.load)(value).map_err(D::Error::custom)
    }
}

/// The state of an edit session of an operation of any kind.
#[derive(Debug)]
pub struct AnyEditState(Box<dyn ErasedState>);

/// A state of any kind: what [`AnyEditState`] holds.
trait ErasedState: Any + Debug + Send + Sync {
    fn boxed_state(&self) -> Box<dyn ErasedState>;
    fn same_state(&self, other: &dyn ErasedState) -> bool;
    fn state_as_any(&self) -> &dyn Any;
}

impl<S: Clone + PartialEq + Debug + Send + Sync + 'static> ErasedState for S {
    fn boxed_state(&self) -> Box<dyn ErasedState> {
        Box::new(self.clone())
    }

    fn same_state(&self, other: &dyn ErasedState) -> bool {
        other.state_as_any().downcast_ref::<S>() == Some(self)
    }

    fn state_as_any(&self) -> &dyn Any {
        self
    }
}

impl AnyEditState {
    /// `state`, as the state of any operation's edit session.
    pub fn new<S: Clone + PartialEq + Debug + Send + Sync + 'static>(state: S) -> Self {
        Self(Box::new(state))
    }

    /// It as an `S`, if it is one.
    pub fn downcast_ref<S: 'static>(&self) -> Option<&S> {
        // (Of what is in the box, not of the box.)
        (*self.0).state_as_any().downcast_ref()
    }
}

impl Clone for AnyEditState {
    fn clone(&self) -> Self {
        Self((*self.0).boxed_state())
    }
}

impl PartialEq for AnyEditState {
    fn eq(&self, other: &Self) -> bool {
        (*self.0).same_state(&*other.0)
    }
}

/// What an edit of an operation changes.
#[derive(Debug, Clone, PartialEq)]
pub struct AnyEdit {
    /// The edited operation, to replace the one edited.
    pub op: Option<AnyOperation>,
    /// The edit session's new state.
    pub state: Option<AnyEditState>,
    /// What the step keeps in its part's cache from now on, if that
    /// changes.
    pub kept: Option<Kept>,
}

/// A kind of operation, as it was registered.
pub struct Kind {
    /// What it is called: its type's name.
    pub name: &'static str,
    /// The icon one of it shows as.
    pub icon: IconOf,
    /// Reads one of it from a value.
    pub load: fn(serde_json::Value) -> Result<AnyOperation, serde_json::Error>,
}

/// The icon the operations of a kind show as.
pub enum IconOf {
    /// One icon for all of them, by name.
    Named(&'static str),
    /// What each says: a surface that is trimmed shows otherwise than
    /// one that is sewn.
    By(fn(&AnyOperation) -> &'static str),
}

/// What a tool makes its operation from.
pub struct ToolContext<'a> {
    /// A name no step of the part has yet, starting as given.
    pub fresh_id: &'a dyn Fn(&str) -> String,
    /// The world the operation will be applied to.
    pub world: &'a World,
    /// The unit the part's lengths are in.
    pub unit: crate::Unit,
    /// What is selected.
    pub selection: &'a [Source],
}

/// A tool of the ribbon: a button that adds an operation.
pub struct Tool {
    /// The tab it is on: `"Modeling"`.
    pub tab: &'static str,
    /// The group of tools it stands with there (`"Create"`); none for
    /// one that stands alone.
    pub group: &'static str,
    /// What it is called.
    pub label: &'static str,
    /// Its icon, by name.
    pub icon: &'static str,
    /// Its key; a small letter stands for the key with shift.
    pub key: char,
    /// What it does, in a sentence.
    pub hint: &'static str,
    /// Whether it is shown small, stacked with the others of its group:
    /// for the many tools of a kind that are reached for less often.
    pub small: bool,
    /// Makes the operation it adds.
    pub make: fn(&ToolContext<'_>) -> AnyOperation,
}

/// Tools of a tab offered as one button of the ribbon, which opens a
/// menu of them: the ways of making one kind of thing that are each a
/// step of its own (a curve through points, a helix, ...).
pub struct Menu {
    /// The tab it is on.
    pub tab: &'static str,
    /// The group of tools it stands with there.
    pub group: &'static str,
    /// What the button is called.
    pub label: &'static str,
    /// Its icon, by name.
    pub icon: &'static str,
    /// What it offers, in a sentence.
    pub hint: &'static str,
    /// Whether it is shown small, stacked with the others of its group.
    pub small: bool,
    /// The labels of the tools of the tab it offers, in order: they show
    /// in the menu, not each on the ribbon.
    pub tools: &'static [&'static str],
}

/// The operations there are: their kinds, their tools, their icons.
#[derive(Default)]
pub struct Registry {
    kinds: BTreeMap<&'static str, Kind>,
    tools: Vec<Tool>,
    menus: Vec<Menu>,
    icons: BTreeMap<&'static str, &'static str>,
}

fn load<T>(value: serde_json::Value) -> Result<AnyOperation, serde_json::Error>
where
    T: ErasedOperation + serde::de::DeserializeOwned,
{
    serde_json::from_value::<T>(value).map(AnyOperation::new)
}

impl Registry {
    /// Tells of the operations of kind `T`, which show as `icon`.
    pub fn kind<T>(&mut self, icon: &'static str)
    where
        T: ErasedOperation + serde::de::DeserializeOwned,
    {
        self.kind_by::<T>(IconOf::Named(icon));
    }

    /// Tells of the operations of kind `T`, each of which shows as what
    /// `icon` says of it.
    pub fn kind_by<T>(&mut self, icon: IconOf)
    where
        T: ErasedOperation + serde::de::DeserializeOwned,
    {
        let name = name_of::<T>();
        self.kinds.insert(
            name,
            Kind {
                name,
                icon,
                load: load::<T>,
            },
        );
    }

    /// Tells of a tool.
    pub fn tool(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    /// Tells of an icon: its name, and its picture as SVG.
    pub fn icon(&mut self, name: &'static str, svg: &'static str) {
        self.icons.insert(name, svg);
    }

    /// Tells of a menu of tools.
    pub fn menu(&mut self, menu: Menu) {
        self.menus.push(menu);
    }

    /// The tools, in the order they were told of.
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// The menus of tools.
    pub fn menus(&self) -> &[Menu] {
        &self.menus
    }

    /// The picture of the icon called `name`, as SVG.
    pub fn svg(&self, name: &str) -> Option<&'static str> {
        self.icons.get(name).copied()
    }

    /// The kinds of operation, by name.
    pub fn kinds(&self) -> impl Iterator<Item = &Kind> {
        self.kinds.values()
    }
}

static INSTALLED: OnceLock<Registry> = OnceLock::new();

/// Makes `registry` the one steps are read by and tools are taken
/// from. Once: what is installed stays.
pub fn install(registry: Registry) -> &'static Registry {
    INSTALLED.get_or_init(|| registry)
}

/// The registry that was installed, if one was.
pub fn installed() -> Option<&'static Registry> {
    INSTALLED.get()
}
