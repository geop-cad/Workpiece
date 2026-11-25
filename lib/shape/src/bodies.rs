//! The bodies an operation works on, and what becomes of them and its
//! tool.

use anyhow::Context;
use workpiece_boolean::{Op, Solid, combine, read};
use workpiece_core::math::Point3;
use workpiece_core::world::topo::{BodyId, BodyKind, FaceId, Topo};

/// What a new solid does to the bodies there are.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Combine {
    /// Nothing: it is a body of its own.
    #[default]
    NewBody,
    /// It is added to them.
    Join,
    /// It is cut out of them.
    Cut,
    /// Only what they have in common with it stays.
    Intersect,
}

impl Combine {
    /// Every way, in the order they are offered.
    pub const ALL: [Combine; 4] = [
        Combine::NewBody,
        Combine::Join,
        Combine::Cut,
        Combine::Intersect,
    ];

    /// What it becomes when what is made turns around to go the other
    /// way: what was joined on is cut out, and what was cut out joined
    /// on.
    pub fn turned(self) -> Combine {
        match self {
            Combine::Join => Combine::Cut,
            Combine::Cut => Combine::Join,
            other => other,
        }
    }

    /// Its name.
    pub fn label(self) -> &'static str {
        match self {
            Combine::NewBody => "New body",
            Combine::Join => "Join",
            Combine::Cut => "Cut",
            Combine::Intersect => "Intersect",
        }
    }
}

/// The named solid bodies of `topo`, in the order they were made.
pub fn solids(topo: &Topo) -> Vec<(BodyId, String)> {
    topo.bodies()
        .iter()
        .filter(|(_, body)| body.kind == BodyKind::Solid)
        .filter_map(|(id, _)| Some((id, topo.name_of(id)?.to_owned())))
        .collect()
}

/// The solid bodies an operation works on: the one `target` names, or
/// every one but its own tool `tool`.
pub fn targets(
    topo: &Topo,
    tool: &str,
    target: &Option<crate::ui::BodyRef>,
) -> anyhow::Result<Vec<(BodyId, String)>> {
    let all = solids(topo);
    match target {
        Some(source) => {
            let found = all.into_iter().find(|(_, name)| name == source.name());
            Ok(vec![found.with_context(|| {
                format!("there is no solid body {:?}", source.name())
            })?])
        }
        None => Ok(all.into_iter().filter(|(_, name)| name != tool).collect()),
    }
}

/// The solid body face `face` is of.
pub fn body_of(topo: &Topo, face: FaceId) -> Option<BodyId> {
    solids(topo)
        .into_iter()
        .map(|(body, _)| body)
        .find(|&body| topo.body_faces(body).is_some_and(|f| f.contains(&face)))
}

/// How far `bodies` reach from `origin` along `direction`: the farthest
/// of their edges' points. `None` without bodies.
pub fn reach(topo: &Topo, bodies: &[BodyId], origin: Point3, direction: Point3) -> Option<f64> {
    let mut farthest: Option<f64> = None;
    for &body in bodies {
        for face in topo.body_faces(body).unwrap_or_default() {
            let loops = topo.face(face).map_or(&[][..], |f| &f.loops);
            let coedges = loops
                .iter()
                .filter_map(|&l| topo.loop_(l))
                .flat_map(|l| &l.coedges);
            for edge in coedges.filter_map(|&c| topo.coedge(c)?.edge_id()) {
                let Some((edge, curve)) = topo
                    .edge(edge)
                    .and_then(|e| Some((e, topo.curve(e.curve)?)))
                else {
                    continue;
                };
                for k in 0..=16 {
                    let t = edge.t0 + (edge.t1 - edge.t0) * f64::from(k) / 16.0;
                    let along = (curve.point_at(t) - origin).dot(direction);
                    farthest = Some(farthest.map_or(along, |f| f.max(along)));
                }
            }
        }
    }
    farthest
}

/// Puts the solid `tool` of operation `id` into `topo` the way `mode`
/// says: as a body of its own, or joined to, cut out of, or intersected
/// with `targets`. A body changed keeps its name; joined bodies become
/// the first of them.
pub fn settle_into(
    topo: &Topo,
    out: &mut Out<'_>,
    id: &str,
    tool: Solid,
    mode: Combine,
    targets: &[(BodyId, String)],
) -> anyhow::Result<()> {
    let op = match mode {
        Combine::NewBody => {
            out.write(&tool, id);
            return Ok(());
        }
        Combine::Join => {
            let mut all = tool;
            let mut name = None;
            for (body, target) in targets {
                let outcome = combine(Op::Union, &read(topo, *body)?, &all)
                    .with_context(|| format!("joining {target}"))?;
                // Bodies it does not reach stay as they are.
                if outcome.touched {
                    out.remove(topo, *body);
                    all = outcome.solid;
                    name.get_or_insert_with(|| target.clone());
                }
            }
            out.write(&all, name.as_deref().unwrap_or(id));
            return Ok(());
        }
        Combine::Cut => Op::Cut,
        Combine::Intersect => Op::Intersect,
    };
    anyhow::ensure!(!targets.is_empty(), "there is no solid body to work on");
    for (body, target) in targets {
        let what = if op == Op::Cut {
            "cutting"
        } else {
            "intersecting"
        };
        let outcome =
            combine(op, &read(topo, *body)?, &tool).with_context(|| format!("{what} {target}"))?;
        if op == Op::Cut && !outcome.touched {
            continue;
        }
        out.remove(topo, *body);
        if !outcome.solid.is_empty() {
            out.write(&outcome.solid, target);
        }
    }
    Ok(())
}

/// The lump of `tool` that begins at its start: of what is left of it
/// outside `stops` (the bodies it runs into), the piece with its face
/// `{id}/start`. Fails if nothing stops it.
pub fn up_to(topo: &Topo, id: &str, tool: &Solid, stops: &[BodyId]) -> anyhow::Result<Solid> {
    let mut rest = tool.clone();
    for &stop in stops {
        rest = combine(Op::Cut, &rest, &read(topo, stop)?)?.solid;
    }
    let (start, end) = (format!("{id}/start"), format!("{id}/end"));
    let has = |lump: &Solid, name: &str| {
        lump.faces.iter().any(|f| {
            f.name
                .as_deref()
                .is_some_and(|n| n == name || n.starts_with(&format!("{name}#")))
        })
    };
    let lump = rest
        .lumps()?
        .into_iter()
        .find(|lump| has(lump, &start))
        .context("the sketch is inside what it should extrude up to")?;
    anyhow::ensure!(!has(&lump, &end), "nothing lies ahead to extrude up to");
    Ok(lump)
}

/// The lump of what `tool` has in common with `targets` that is nearest
/// to `origin` along `direction`: what a cut up to the next face removes.
pub fn first_common(
    topo: &Topo,
    tool: &Solid,
    targets: &[(BodyId, String)],
    origin: Point3,
    direction: Point3,
) -> anyhow::Result<Solid> {
    let mut lumps: Vec<(f64, Solid)> = Vec::new();
    for (body, _) in targets {
        let common = combine(Op::Intersect, tool, &read(topo, *body)?)?.solid;
        for lump in common.lumps()? {
            let near = lump
                .vertices
                .iter()
                .map(|v| (v.point - origin).dot(direction))
                .fold(f64::INFINITY, f64::min);
            lumps.push((near, lump));
        }
    }
    let first = lumps.iter().map(|l| l.0).fold(f64::INFINITY, f64::min);
    anyhow::ensure!(first.is_finite(), "nothing lies ahead to cut");
    // (What begins as near as the nearest is of the same wall: a
    // profile across a gap meets the wall on both sides of it.)
    let (lo, hi) = tool.extent();
    let within = (hi - lo).length() * 1e-6;
    let mut all = lumps.into_iter().filter(|l| l.0 <= first + within);
    let mut cut = all.next().context("nothing lies ahead to cut")?.1;
    for (_, more) in all {
        cut = combine(Op::Union, &cut, &more)?.solid;
    }
    Ok(cut)
}

impl workpiece_core::Choices for Combine {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }
}

/// The named solid bodies of `topo` that what `names` refer to is of —
/// a body, or a face, an edge or a vertex of one — in the order they
/// were made. Only those are read of by what goes through them for
/// what it was told to work on.
pub fn solids_with<S: AsRef<str>>(topo: &Topo, names: &[S]) -> Vec<(BodyId, String)> {
    let mut owners: Vec<BodyId> = names
        .iter()
        .filter_map(|name| topo.owner(topo.referred(name.as_ref())?))
        .collect();
    owners.sort();
    owners.dedup();
    owners
        .into_iter()
        .filter(|&body| topo.body(body).is_some_and(|b| b.kind == BodyKind::Solid))
        .filter_map(|body| Some((body, topo.name_of(body)?.to_owned())))
        .collect()
}

pub use workpiece_boolean::Out;
