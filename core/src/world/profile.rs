//! Profiles: what a sketch leaves in the world, for features (an extrude,
//! a revolve) to build on.
//!
//! A profile is plain NURBS geometry in a plane, like the topology: the
//! lines, arcs and constraints it was drawn with stay in the sketch. Its
//! vertices and edges carry the same tolerances as topological ones (a
//! disk around a vertex, a band around an edge, in the plane), and names
//! that survive edits of the sketch, so what is built from them can be
//! named after them.
//!
//! # Areas and their names
//!
//! The curves of a sketch cut the plane into *regions*. Regions are
//! derived: a new line across one makes it two, so a region has no id of
//! its own to refer to it by. What refers to a region (an extrude of one
//! area of a sketch) names it by its boundary instead, with
//! [`AreaAnchor`]s: "the region to the left of entity `e7`, 40% along
//! it". The entities' names are stable, so the anchors survive
//! everything that leaves that stretch of boundary in place: dimension
//! changes, unrelated curves, splits elsewhere. A region is referred to by
//! an anchor per piece of its boundary; if they no longer agree (the
//! region was split in two) or one is gone (its curve was trimmed away
//! there), [`Profile::region_at`] fails rather than guess.
//!
//! What a feature takes is an [`AreaPick`]: a region by its boundary, as
//! the view's clicks make them, or by a point inside it, as a script
//! says it — a place in the plane needs no ids, so it survives curves
//! added anywhere in the sketch (ids shift when a script draws earlier
//! curves) as long as the region stays around the point.

use super::datum::DatumPlane;
use super::topo::Curve2;
use crate::math::Point2;

/// A point of a profile where edges end.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileVertex {
    /// Its name within the profile, e.g. `"p3"`.
    pub name: String,
    /// Its position in the plane.
    pub point: Point2,
    /// Radius of the disk around `point` that the true vertex lies in. It
    /// covers the ends of the edges that meet here, with their tolerances.
    pub tolerance: f64,
}

/// A bounded piece `[t0, t1]` of a curve in the profile's plane.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileEdge {
    /// Its name within the profile: the entity it is (a piece of), and for
    /// a piece the vertices it runs between, e.g. `"e7"` or
    /// `"e7[p3..x(e2,e7)]"`.
    pub name: String,
    /// The sketch entity it is (a piece of), e.g. `"e7"`.
    pub entity: String,
    /// The stretch of the entity it is, as fractions along it from 0 to 1
    /// (by length for a line, by angle for an arc or a circle).
    pub span: (f64, f64),
    /// The curve, in the plane's coordinates.
    pub curve: Curve2,
    /// Start parameter (at `start`).
    pub t0: f64,
    /// End parameter (at `end`).
    pub t1: f64,
    /// Index of the vertex at `t0`.
    pub start: usize,
    /// Index of the vertex at `t1`; equal to `start` for a closed edge.
    pub end: usize,
    /// Half-width of the band around the curve that the true edge lies in.
    pub tolerance: f64,
}

/// One way along an edge: the edge's index, and whether against its
/// direction.
pub type EdgeUse = (usize, bool);

/// A bounded region of the plane that the profile's edges enclose.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRegion {
    /// Its outer boundary, walked counter-clockwise (the region on the
    /// left).
    pub outer: Vec<EdgeUse>,
    /// The boundaries of its holes, each walked clockwise (the region on
    /// the left, again).
    pub holes: Vec<Vec<EdgeUse>>,
    /// Its area, measured on its sampled outline: close, not exact.
    pub area: f64,
}

/// A stretch of a region's boundary, by which the region is referred to:
/// the region on one side of a sketch entity, at a place along it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AreaAnchor {
    /// The entity's name, e.g. `"e7"`.
    pub entity: String,
    /// Whether the region is to the left of the entity as it runs (to the
    /// right if not).
    pub left: bool,
    /// Where along the entity, as a fraction from 0 to 1.
    pub at: f64,
}

/// One area of a sketch, as a feature takes it.
///
/// Written as a point, `{"inside": [u, v]}`, or as a list of anchors,
/// `[{"entity": "e7", "left": true, "at": 0.5}, ...]`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum AreaPick {
    /// The region around a point of the plane, in its (u, v): the
    /// smallest one the point is inside (in none of its holes). The point
    /// is best well inside, not on a curve.
    Inside {
        /// The point.
        inside: Point2,
    },
    /// The region these anchors are on the boundary of (see
    /// [`Profile::region_at`]).
    Bounded(Vec<AreaAnchor>),
}

impl AreaPick {
    /// The region around `(u, v)`.
    pub fn inside(u: f64, v: f64) -> Self {
        Self::Inside {
            inside: Point2::new(u, v),
        }
    }
}

/// Why anchors do not name a region.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AreaError {
    /// No anchors were given.
    #[error("no anchors: nothing says which area")]
    NoAnchors,
    /// An anchor's entity is not in the profile, or not at that place:
    /// it was deleted, or trimmed away there.
    #[error("the area's boundary {entity:?} is gone at {at}")]
    Lost {
        /// The entity.
        entity: String,
        /// Where along it.
        at: f64,
    },
    /// The side of the entity the anchor names is outside every region.
    #[error("the side of {entity:?} that bounded the area is open now")]
    Open {
        /// The entity.
        entity: String,
    },
    /// The anchors are on the boundaries of different regions: the area
    /// was split.
    #[error("the area was split in {0}: pick the pieces that are meant")]
    Split(usize),
}

/// The curves of a sketch, in its plane.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// The plane the curves lie in; their coordinates are its.
    pub plane: DatumPlane,
    /// Where edges end.
    pub vertices: Vec<ProfileVertex>,
    /// The edges: the sketch's curves, cut where they cross.
    pub edges: Vec<ProfileEdge>,
    /// The bounded regions the edges enclose.
    pub regions: Vec<ProfileRegion>,
}

impl Profile {
    /// An empty profile on `plane`.
    pub fn new(plane: DatumPlane) -> Self {
        Self {
            plane,
            vertices: Vec::new(),
            edges: Vec::new(),
            regions: Vec::new(),
        }
    }

    /// The vertices where only one edge ends: the outline is open
    /// there. Each with the name of its edge's entity.
    pub fn open_ends(&self) -> Vec<(usize, &str)> {
        let mut seen: Vec<(usize, Option<&str>)> = vec![(0, None); self.vertices.len()];
        for edge in &self.edges {
            for v in [edge.start, edge.end] {
                seen[v].0 += 1;
                seen[v].1 = Some(&edge.entity);
            }
        }
        seen.iter()
            .enumerate()
            .filter(|(_, (n, _))| *n == 1)
            .map(|(v, (_, entity))| (v, entity.unwrap_or("")))
            .collect()
    }

    /// For a message that a sketch has no closed area: where its
    /// outline is open (what ends there, and where), if anywhere.
    pub fn open_note(&self) -> String {
        let open = self.open_ends();
        if open.is_empty() {
            return String::new();
        }
        let places: Vec<String> = open
            .iter()
            .take(6)
            .map(|&(v, entity)| {
                let vertex = &self.vertices[v];
                format!(
                    "{} ends at {} ({:.4}, {:.4})",
                    entity, vertex.name, vertex.point.x, vertex.point.y
                )
            })
            .collect();
        format!(
            ": the outline is open where {}{}",
            places.join("; "),
            if open.len() > 6 { "; …" } else { "" }
        )
    }

    /// The region whose boundary walks `use_` (an edge, one way).
    fn region_of(&self, use_: EdgeUse) -> Option<usize> {
        self.regions
            .iter()
            .position(|r| r.outer.contains(&use_) || r.holes.iter().any(|h| h.contains(&use_)))
    }

    /// The anchors that name region `region`: one per piece of its
    /// boundary, in its middle.
    pub fn anchors(&self, region: usize) -> Vec<AreaAnchor> {
        let Some(region) = self.regions.get(region) else {
            return Vec::new();
        };
        let boundary = region.outer.iter().chain(region.holes.iter().flatten());
        boundary
            .filter_map(|&(edge, reversed)| {
                let edge = self.edges.get(edge)?;
                Some(AreaAnchor {
                    entity: edge.entity.clone(),
                    // Walked along the edge, the region is on its left.
                    left: !reversed,
                    at: (edge.span.0 + edge.span.1) / 2.0,
                })
            })
            .collect()
    }

    /// The region `anchors` name: the one every anchor is on the boundary
    /// of. Fails, rather than guess, when an anchor's stretch of boundary
    /// is gone or they no longer agree.
    pub fn region_at(&self, anchors: &[AreaAnchor]) -> Result<usize, AreaError> {
        let mut found: Vec<usize> = Vec::new();
        for anchor in anchors {
            let lost = || AreaError::Lost {
                entity: anchor.entity.clone(),
                at: anchor.at,
            };
            let edge = self
                .edges
                .iter()
                .position(|e| {
                    e.entity == anchor.entity && e.span.0 <= anchor.at && anchor.at <= e.span.1
                })
                .ok_or_else(lost)?;
            let region = self
                .region_of((edge, !anchor.left))
                .ok_or_else(|| AreaError::Open {
                    entity: anchor.entity.clone(),
                })?;
            if !found.contains(&region) {
                found.push(region);
            }
        }
        match found[..] {
            [] => Err(AreaError::NoAnchors),
            [region] => Ok(region),
            _ => Err(AreaError::Split(found.len())),
        }
    }

    /// Whether every edge end lies within its vertex's tolerance, with the
    /// edge's own tolerance to spare (the rule vertices and edges of the
    /// topology keep).
    pub fn is_closed_within_tolerance(&self) -> bool {
        self.edges.iter().all(|e| {
            [(e.t0, e.start), (e.t1, e.end)].into_iter().all(|(t, v)| {
                self.vertices.get(v).is_some_and(|v| {
                    e.curve.point_at(t).distance(v.point) + e.tolerance <= v.tolerance
                })
            })
        })
    }
}
