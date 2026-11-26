//! Snapping in the scene: what a point dragged or placed in three
//! dimensions lands on — a point, the middle of an edge, a centre, where
//! two edges cross as seen, the foot of a perpendicular, onto an edge —
//! and the grid, whose spacing follows the zoom as the sketcher's does.
//!
//! What is snapped to is found by the host (it knows the scene: the
//! bodies, the sketches, the datums) together with what a
//! [`Space`](super::Space) offers of its own ([`SnapTargets`]); the
//! space is told the result with each event ([`SpaceEvent::snap`](super::SpaceEvent)).

use crate::math::Point3;

/// What a point can snap to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SnapKind {
    /// A point: a vertex, a sketch's point, a datum's origin.
    Point,
    /// The middle of an edge.
    Middle,
    /// The centre of a circle or an arc, or of a face.
    Centre,
    /// A quadrant point of a circle: where it is farthest along its
    /// plane's axes.
    Quad,
    /// Where two edges cross, as they are seen.
    Crossing,
    /// The foot of the perpendicular from where the drag started onto
    /// an edge.
    Perpendicular,
    /// The point of an edge nearest the pointer.
    Edge,
    /// The grid: whole steps of its spacing.
    Grid,
}

impl SnapKind {
    /// All of them, in the order they are tried (the grid last).
    pub const ALL: [SnapKind; 8] = [
        SnapKind::Point,
        SnapKind::Middle,
        SnapKind::Centre,
        SnapKind::Quad,
        SnapKind::Crossing,
        SnapKind::Perpendicular,
        SnapKind::Edge,
        SnapKind::Grid,
    ];

    /// What it is called.
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Point => "Point",
            SnapKind::Middle => "Middle",
            SnapKind::Centre => "Centre",
            SnapKind::Quad => "Quadrant",
            SnapKind::Crossing => "Crossing",
            SnapKind::Perpendicular => "Perpendicular",
            SnapKind::Edge => "On edge",
            SnapKind::Grid => "Grid",
        }
    }

    /// The badge a snap of it shows by, in the sketcher's way: `◎` a
    /// point, `M` a middle, `×` a crossing, `on` an edge.
    pub fn badge(self) -> &'static str {
        match self {
            SnapKind::Point => "◎",
            SnapKind::Middle => "M",
            SnapKind::Centre => "C",
            SnapKind::Quad => "Q",
            SnapKind::Crossing => "×",
            SnapKind::Perpendicular => "⊥",
            SnapKind::Edge => "on",
            SnapKind::Grid => "#",
        }
    }

    /// The name of its icon.
    pub fn icon(self) -> &'static str {
        match self {
            SnapKind::Point => "snap-point",
            SnapKind::Middle => "snap-middle",
            SnapKind::Centre => "snap-centre",
            SnapKind::Quad => "snap-quad",
            SnapKind::Crossing => "snap-crossing",
            SnapKind::Perpendicular => "snap-perpendicular",
            SnapKind::Edge => "snap-edge",
            SnapKind::Grid => "snap-grid",
        }
    }

    fn index(self) -> usize {
        SnapKind::ALL
            .iter()
            .position(|k| *k == self)
            .unwrap_or_default()
    }
}

/// Which kinds of snap are taken: each by itself, and all at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapKinds {
    /// Whether anything snaps at all.
    pub on: bool,
    /// Whether each kind is taken, in the order of [`SnapKind::ALL`].
    pub kinds: [bool; 8],
}

impl Default for SnapKinds {
    /// All but the perpendicular, which pulls where it is not wanted
    /// more often than not.
    fn default() -> Self {
        let mut kinds = [true; 8];
        kinds[SnapKind::Perpendicular.index()] = false;
        Self { on: true, kinds }
    }
}

impl SnapKinds {
    /// Whether a snap of `kind` is taken.
    pub fn takes(&self, kind: SnapKind) -> bool {
        self.on && self.kinds[kind.index()]
    }

    /// Whether `kind` is switched on (whatever the master switch is).
    pub fn has(&self, kind: SnapKind) -> bool {
        self.kinds[kind.index()]
    }

    /// The same, with `kind` switched `on` or off.
    #[must_use]
    pub fn with(mut self, kind: SnapKind, on: bool) -> Self {
        self.kinds[kind.index()] = on;
        self
    }
}

/// Where a point snapped to, and to what kind of thing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// Where it lands.
    pub at: Point3,
    /// What it is on.
    pub kind: SnapKind,
}

/// What a space offers to be snapped to of its own (what it edits, but
/// for what is being moved): points of each kind, and edges as straight
/// pieces.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapTargets {
    /// The points, each with what it is: a [`SnapKind::Point`], a
    /// [`SnapKind::Middle`], a [`SnapKind::Centre`] or a
    /// [`SnapKind::Quad`].
    pub points: Vec<(Point3, SnapKind)>,
    /// The edges, as straight pieces between their ends.
    pub segments: Vec<[Point3; 2]>,
}

/// How far apart the lines of the grid are where a pixel is `pixel`
/// long: 1, 2 or 5 of a power of ten, the least that leaves them some
/// pixels apart. The sketcher's grid, and the scene's.
pub fn grid_step(pixel: f64) -> f64 {
    const APART_PX: f64 = 14.0;
    let least = APART_PX * pixel;
    let ten = libm::pow(10f64, libm::log10(least).floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|k| k * ten)
        .find(|step| *step >= least)
        .unwrap_or(10.0 * ten)
}

/// `value` to the nearest whole number of `step`s.
pub fn to_grid(value: f64, step: f64) -> f64 {
    if step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

/// `p`, a point of the plane through it with unit `normal`, moved in
/// that plane onto the grid of spacing `step`: the two coordinates the
/// plane runs along most are whole steps, the third keeps it in the
/// plane.
pub fn grid_in_plane(p: Point3, normal: Point3, step: f64) -> Point3 {
    let n = normal.to_array();
    let Some(k) = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())) else {
        return p;
    };
    if n[k].abs() < 1e-12 {
        return p;
    }
    let mut q = p.to_array();
    for (i, c) in q.iter_mut().enumerate() {
        if i != k {
            *c = to_grid(*c, step);
        }
    }
    // Back into the plane along the third axis.
    let off: f64 = (0..3).map(|i| n[i] * (q[i] - p[i])).sum();
    q[k] -= off / n[k];
    Point3::from_array(q)
}

/// How far to travel along the unit `axis` from `origin` instead of
/// `travel` so that the point lands on the grid of spacing `step`: on a
/// whole step of the world's coordinate along it, where the axis is one
/// of the world's; else a whole number of steps.
pub fn grid_along(origin: Point3, axis: Point3, travel: f64, step: f64) -> f64 {
    let a = axis.to_array();
    let world = (0..3).find(|&i| a[i].abs() > 1.0 - 1e-9);
    match world {
        Some(i) => {
            let to = origin[i] + a[i] * travel;
            (to_grid(to, step) - origin[i]) / a[i]
        }
        None => to_grid(travel, step),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_follows_the_zoom_and_keeps_to_planes_and_axes() {
        assert!((grid_step(0.01) - 0.2).abs() < 1e-12);
        assert!((grid_step(0.1) - 2.0).abs() < 1e-12);
        let p = grid_in_plane(Point3::new(1.3, 2.6, 5.0), Point3::Z, 1.0);
        assert!(p.abs_diff_eq(Point3::new(1.0, 3.0, 5.0), 1e-12));
        let tilted = Point3::new(0.0, 0.0, 1.0) + Point3::X * 0.2;
        let n = tilted.normalize();
        let q = grid_in_plane(Point3::new(1.3, 2.6, 0.0), n, 1.0);
        assert!((q - Point3::new(1.3, 2.6, 0.0)).dot(n).abs() < 1e-9);
        assert!((q.x - 1.0).abs() < 1e-12 && (q.y - 3.0).abs() < 1e-12);
        let s = grid_along(Point3::new(0.3, 0.0, 0.0), Point3::X, 1.1, 0.5);
        assert!((0.3 + s - 1.5).abs() < 1e-12);
    }
}
