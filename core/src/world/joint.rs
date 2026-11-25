//! Joints: how the instances of an assembly are held to each other.
//!
//! A joint connects two *connectors*, each a frame on an instance (or on
//! the assembly itself): the joint's kind says how the second frame may
//! move against the first. Every kind is told along the connector's z,
//! the joint's axis.

use crate::pose::Pose;

/// How the second connector of a joint may move against the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum JointKind {
    /// Not at all: the frames are one.
    #[default]
    Fastened,
    /// It turns around the axis.
    Revolute,
    /// It slides along the axis.
    Slider,
    /// It turns around the axis and slides along it.
    Cylindrical,
    /// It slides in the plane across the axis and turns in it.
    Planar,
    /// It turns any way around the first's origin.
    Ball,
}

impl JointKind {
    /// Every kind, in the order they are offered.
    pub const ALL: [JointKind; 6] = [
        JointKind::Fastened,
        JointKind::Revolute,
        JointKind::Slider,
        JointKind::Cylindrical,
        JointKind::Planar,
        JointKind::Ball,
    ];

    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            JointKind::Fastened => "Fastened",
            JointKind::Revolute => "Revolute",
            JointKind::Slider => "Slider",
            JointKind::Cylindrical => "Cylindrical",
            JointKind::Planar => "Planar",
            JointKind::Ball => "Ball",
        }
    }

    /// Whether it turns around its axis: whether an angle limits it.
    pub fn turns(self) -> bool {
        matches!(self, JointKind::Revolute | JointKind::Cylindrical)
    }

    /// Whether it slides along its axis: whether a distance limits it.
    pub fn slides(self) -> bool {
        matches!(self, JointKind::Slider | JointKind::Cylindrical)
    }
}

/// One end of a joint: a frame on an instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    /// The instance it is on; `None` for the assembly itself, which
    /// does not move.
    pub instance: Option<String>,
    /// The frame, in that instance's own coordinates.
    pub frame: Pose,
}

/// How far a joint may go: its least and greatest.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    /// The angle around the axis, in radians.
    pub angle: Option<(f64, f64)>,
    /// The distance along the axis.
    pub distance: Option<(f64, f64)>,
}

/// A joint between two connectors.
#[derive(Debug, Clone, PartialEq)]
pub struct Joint {
    /// The step that made it.
    pub name: String,
    /// How the second connector may move against the first.
    pub kind: JointKind,
    /// The first connector.
    pub a: Connector,
    /// The second.
    pub b: Connector,
    /// How far it may go.
    pub limits: Limits,
}

/// How one joint's travel follows another's.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CouplingKind {
    /// Two joints that turn: the second turns `ratio` times as far as
    /// the first (against it, for a negative ratio: two gears in mesh).
    Gear {
        /// Turns of the second per turn of the first.
        ratio: f64,
    },
    /// A joint that turns and one that slides: the second slides
    /// `radius` for every radian the first turns (a pinion of that pitch
    /// radius on a rack).
    RackAndPinion {
        /// The pinion's pitch radius.
        radius: f64,
    },
    /// A joint that turns and one that slides: the second slides `lead`
    /// for every whole turn of the first (a screw and its nut; both may
    /// be the one cylindrical joint).
    Screw {
        /// The travel per turn.
        lead: f64,
    },
    /// Two joints that slide: the second slides `ratio` times as far as
    /// the first.
    Linear {
        /// Travel of the second per travel of the first.
        ratio: f64,
    },
}

impl CouplingKind {
    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            CouplingKind::Gear { .. } => "Gear",
            CouplingKind::RackAndPinion { .. } => "Rack and pinion",
            CouplingKind::Screw { .. } => "Screw",
            CouplingKind::Linear { .. } => "Linear",
        }
    }

    /// Whether its first joint has to turn (else slide), and its second.
    pub fn turns(self) -> (bool, bool) {
        match self {
            CouplingKind::Gear { .. } => (true, true),
            CouplingKind::RackAndPinion { .. } | CouplingKind::Screw { .. } => (true, false),
            CouplingKind::Linear { .. } => (false, false),
        }
    }

    /// How far the second joint goes (radians or lengths) for one of the
    /// first (a radian or a length).
    pub fn rate(self) -> f64 {
        match self {
            CouplingKind::Gear { ratio } | CouplingKind::Linear { ratio } => ratio,
            CouplingKind::RackAndPinion { radius } => radius,
            CouplingKind::Screw { lead } => lead / std::f64::consts::TAU,
        }
    }
}

/// A mechanism: two joints whose travels go together.
#[derive(Debug, Clone, PartialEq)]
pub struct Coupling {
    /// The step that made it.
    pub name: String,
    /// How the second joint follows the first.
    pub kind: CouplingKind,
    /// The first joint, by name.
    pub a: String,
    /// The second.
    pub b: String,
}

impl crate::Choices for JointKind {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }
}
