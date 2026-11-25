//! The modelling world: one boundary-representation [`Topo`], plus the
//! named datums and profiles that features are built from, and the
//! instances of other parts placed in it.

pub mod annotation;
pub mod datum;
pub mod instance;
pub mod joint;
pub mod look;
mod mass;
pub mod profile;
pub mod thread;
pub mod topo;
pub mod view;
pub mod visibility;

use std::collections::BTreeMap;

pub use annotation::{
    Annotation, Callout, Characteristic, Measured, Note, Numbers, Opaque, SketchOf, Tolerance,
};
pub use datum::{Datum, DatumAxis, DatumFrame, DatumPlane};
pub use instance::Instance;
pub use joint::{Connector, Coupling, CouplingKind, Joint, JointKind, Limits};
pub use look::{Look, Shade, Shades};
pub use mass::{MassProperties, Spread, Weighed};
pub use profile::{
    AreaAnchor, AreaError, AreaPick, Profile, ProfileEdge, ProfileRegion, ProfileVertex,
};
pub use thread::Thread;
pub use topo::Topo;
pub use view::{Cutting, Sectioned, Viewpoint};
pub use visibility::Visibility;

/// Name of the datum frame every world starts with.
pub const ORIGIN: &str = "origin";

/// The size reference geometry is drawn at in a world that is empty or
/// small (see [`World::datum_size`]).
pub const DATUM_SIZE: f64 = 10.0;

/// Everything a part is made of. Maps are ordered so iteration (and thus
/// anything derived from it) is deterministic.
#[derive(Debug, Clone)]
pub struct World {
    /// The B-rep: topology, NURBS geometry and persistent names.
    pub topo: Topo,
    /// Reference geometry, by name. Starts with the global frame, [`ORIGIN`].
    pub datums: BTreeMap<String, Datum>,
    /// Profiles, by name.
    pub profiles: BTreeMap<String, Profile>,
    /// Other parts placed in this one, by name.
    pub instances: BTreeMap<String, Instance>,
    /// The joints between the instances, in the order they were made.
    pub joints: Vec<Joint>,
    /// The mechanisms between the joints: gears, racks, screws.
    pub couplings: Vec<Coupling>,
    /// What is said of the model on it — dimensions, datum features,
    /// geometric tolerances — by name.
    pub annotations: BTreeMap<String, Annotation>,
    /// How bodies and faces look that were given a look of their own,
    /// by their names (see [`Look`]).
    pub looks: BTreeMap<String, Look>,
    /// The threads that are said of faces, not cut into them, by name
    /// (see [`Thread`]).
    pub threads: BTreeMap<String, Thread>,
    /// The named views of the model, by name: how it is looked at.
    pub views: BTreeMap<String, Viewpoint>,
    /// Where the model is cut open to show its inside, by name.
    pub sections: BTreeMap<String, Cutting>,
    /// The mass properties steps ask for, by name.
    pub masses: BTreeMap<String, MassProperties>,
    /// What the part is made of, where nothing else is said: a
    /// material, by its name in the library.
    pub material: Option<String>,
    /// What a parts list calls the part where a catalogue names it: its
    /// designation (`Deep groove ball bearing 6204`).
    pub designation: Option<String>,
    /// How a section through the part treats it.
    pub sectioned: Sectioned,
    /// What the world was built from, as a number that is the same for
    /// the same program, parameters and parts, wherever the instances
    /// are placed: what is drawn of the world itself can be kept under
    /// it. Zero for a world not built by an engine.
    pub stamp: u64,
}

impl Default for World {
    fn default() -> Self {
        Self {
            topo: Topo::new(),
            datums: BTreeMap::from([(ORIGIN.to_owned(), Datum::Frame(DatumFrame::global()))]),
            profiles: BTreeMap::new(),
            instances: BTreeMap::new(),
            joints: Vec::new(),
            couplings: Vec::new(),
            annotations: BTreeMap::new(),
            looks: BTreeMap::new(),
            threads: BTreeMap::new(),
            views: BTreeMap::new(),
            sections: BTreeMap::new(),
            masses: BTreeMap::new(),
            material: None,
            designation: None,
            sectioned: Sectioned::Cut,
            stamp: 0,
        }
    }
}

use crate::math::Point3;

impl World {
    /// What is said of the model as `name`: of its own annotations, or —
    /// `part/name`, by the name a part is placed under — one of a placed
    /// part's (on down), where that part is.
    pub fn said(&self, name: &str) -> Option<std::borrow::Cow<'_, Annotation>> {
        if let Some(own) = self.annotations.get(name) {
            return Some(std::borrow::Cow::Borrowed(own));
        }
        let (placed, rest) = name.split_once('/')?;
        let instance = self.instances.get(placed)?;
        let inner = instance.world.said(rest)?;
        Some(std::borrow::Cow::Owned(inner.placed(instance.pose)))
    }

    /// The box around everything it has that has a size — its bodies'
    /// vertices, its sketches' points, and what is placed in it — if
    /// there is anything.
    pub fn extent(&self) -> Option<(Point3, Point3)> {
        let mut points: Vec<Point3> = self.topo.vertices().iter().map(|v| v.1.point).collect();
        for profile in self.profiles.values() {
            let pose = profile.plane.pose();
            points.extend(
                profile
                    .vertices
                    .iter()
                    .map(|v| pose.transform_point(v.point.extend(0.0))),
            );
        }
        for instance in self.instances.values() {
            if let Some((lo, hi)) = instance.world.extent() {
                points.extend((0..8).map(|k| {
                    let corner =
                        |bit: usize, lo: f64, hi: f64| if k >> bit & 1 == 0 { lo } else { hi };
                    instance.pose.transform_point(Point3::new(
                        corner(0, lo.x, hi.x),
                        corner(1, lo.y, hi.y),
                        corner(2, lo.z, hi.z),
                    ))
                }));
            }
        }
        let first = *points.first()?;
        Some(
            points
                .iter()
                .fold((first, first), |(lo, hi), &p| (lo.min(p), hi.max(p))),
        )
    }

    /// The size its reference geometry is drawn (and picked) at: the
    /// length of a frame's arrows and half the side of a plane's square,
    /// in the world's unit. [`DATUM_SIZE`] where there is little or
    /// nothing yet, and a part of the model's size as that grows, so
    /// that datums are neither lost in a large model nor hide a small
    /// one.
    pub fn datum_size(&self) -> f64 {
        let across = self.extent().map_or(0.0, |(lo, hi)| (hi - lo).length());
        (across * 0.2).max(DATUM_SIZE)
    }

    /// A world holding only the [`ORIGIN`] frame.
    pub fn new() -> Self {
        Self::default()
    }
}

// Worlds are built on worker threads and shared with the interface.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<World>();
};
