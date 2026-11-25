//! References that outlive their names.
//!
//! What a step refers to of a body it names by the body's history:
//! `extrude2/side/e7`, the side swept from the sketch's seventh curve.
//! Such a name is lost when the history upstream is changed so that
//! the same piece comes about another way (the sketch redrawn, a step
//! put in before). So that the step still finds it, where each piece a
//! step refers to *is* is kept in the part's cache whenever the name
//! resolves ([`settle`] notes it); when the name resolves no longer,
//! the piece of the same kind that is now nearest to there is taken in
//! its place, if one is clearly it — nearer by half than any other, and
//! not far from where the lost one was.

use shapecraft_core::math::Point3;
use shapecraft_core::world::topo::{CoedgeKind, EntityId, Topo};
use shapecraft_core::{AnyOperation, Cache, Source, World};

/// A reference of a step that lost what it named and was pointed at
/// what is now where that was.
#[derive(Debug, Clone, PartialEq)]
pub struct Repair {
    /// The step.
    pub step: String,
    /// What it referred to, by a name that is no more.
    pub from: Source,
    /// What it refers to instead.
    pub to: Source,
}

/// The owner, in a part's cache, of where what the step `id` refers to
/// is.
pub(crate) fn owner(id: &str) -> String {
    format!("{id}@refs")
}

/// What kind of piece `id` is, as a number to keep, and a point of it:
/// a vertex's place, an edge's middle, the middle of what goes around
/// a face.
fn place(topo: &Topo, id: EntityId) -> Option<(f64, Point3)> {
    let middle = |edge| {
        let e = topo.edge(edge)?;
        Some(topo.curve(e.curve)?.point_at(0.5 * (e.t0 + e.t1)))
    };
    match id {
        EntityId::Vertex(v) => Some((0.0, topo.vertex(v)?.point)),
        EntityId::Edge(e) => Some((1.0, middle(e)?)),
        EntityId::Face(f) => {
            let face = topo.face(f)?;
            let around = face
                .loops
                .iter()
                .filter_map(|&l| topo.loop_(l))
                .flat_map(|l| &l.coedges);
            let points: Vec<Point3> = around
                .filter_map(|&c| match topo.coedge(c)?.kind {
                    CoedgeKind::Edge { edge, .. } => middle(edge),
                    CoedgeKind::Pole { vertex, .. } => Some(topo.vertex(vertex)?.point),
                })
                .collect();
            let n = points.len() as f64;
            (n > 0.0).then(|| (2.0, points.into_iter().sum::<Point3>() / n))
        }
        _ => None,
    }
}

/// The name of the piece of kind `kind` that is where `at` was, if one
/// clearly is.
fn found(topo: &Topo, kind: f64, at: Point3) -> Option<String> {
    let mut nearest: Vec<(f64, &str)> = topo
        .names()
        .iter()
        .filter_map(|(name, id)| {
            let (its, point) = place(topo, id)?;
            (its == kind).then(|| ((point - at).length(), name))
        })
        .collect();
    nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (distance, name) = *nearest.first()?;
    // How large what there is is: what "not far" is measured by.
    let points = topo.vertices().iter().map(|(_, v)| v.point);
    let (lo, hi) = points.fold(
        (
            Point3::splat(f64::INFINITY),
            Point3::splat(f64::NEG_INFINITY),
        ),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    );
    let size = (hi - lo).length();
    let next = nearest.get(1).map_or(f64::INFINITY, |n| n.0);
    let clearly = distance <= 1e-9 * size.max(1.0) || 2.0 * distance < next;
    (size.is_finite() && distance <= 0.1 * size && clearly).then(|| name.to_owned())
}

/// Notes in `cache` where what `op` refers to is in `world`, the world
/// it is about to be applied to. Where a name it refers to resolves no
/// longer, and what it named was noted before, the piece that is there
/// now is referred to instead: the operation so pointed is returned
/// (nothing, if all its names resolve or nothing is found for them).
pub(crate) fn settle(
    op: &AnyOperation,
    world: &World,
    cache: &mut Cache,
    found_again: &mut Vec<Repair>,
) -> Option<AnyOperation> {
    let topo = &world.topo;
    let owner = owner(op.id());
    let mut noted: Vec<(String, Vec<f64>)> = Vec::new();
    let mut pointed: Option<AnyOperation> = None;
    for source in op.references() {
        let Source::Topology(name) = &source else {
            continue;
        };
        let kept = cache.get(&format!("{owner}/{name}")).map(<[f64]>::to_vec);
        let now = match topo.referred(name) {
            Some(id) => place(topo, id),
            None => {
                // Lost: what is where it was.
                let lost = kept.as_deref().filter(|k| k.len() == 4);
                let other = lost.and_then(|k| found(topo, k[0], Point3::new(k[1], k[2], k[3])));
                other.and_then(|other| {
                    let at = place(topo, topo.referred(&other)?);
                    let to = Source::Topology(other);
                    pointed
                        .get_or_insert_with(|| op.clone())
                        .retarget(&source, &to);
                    found_again.push(Repair {
                        step: op.id().to_owned(),
                        from: source.clone(),
                        to,
                    });
                    at
                })
            }
        };
        // (Where it is now; or, still lost, where it was.)
        let entry = now.map(|(kind, p)| vec![kind, p.x, p.y, p.z]).or(kept);
        noted.extend(entry.map(|e| (name.clone(), e)));
    }
    if !noted.is_empty() || cache.of(&owner).next().is_some() {
        cache.replace(&owner, noted);
    }
    pointed
}
