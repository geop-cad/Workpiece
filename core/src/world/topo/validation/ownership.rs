//! Each child entity belongs to at most one owner and appears once in it,
//! so the hierarchy is a tree over coedges, loops, faces, shells and lumps
//! (edges and vertices are the shared part).
//!
//! Each check runs from the owner's side: every child of this owner must
//! appear exactly once among all owners of that kind.

use super::ValidationError;
use crate::world::topo::{BodyId, EntityId, FaceId, LoopId, LumpId, ShellId, Topo};

fn once(id: EntityId, count: usize) -> Result<(), ValidationError> {
    if count == 1 {
        Ok(())
    } else {
        Err(ValidationError::MultipleOwners { id, count })
    }
}

/// Coedges of a loop.
pub(super) fn loop_coedges(topo: &Topo, id: LoopId) -> Result<(), ValidationError> {
    for &c in &topo.loops[id].coedges {
        let count = topo
            .loops
            .iter()
            .map(|(_, l)| l.coedges.iter().filter(|&&x| x == c).count())
            .sum();
        once(c.into(), count)?;
    }
    Ok(())
}

/// Loops of a face.
pub(super) fn face_loops(topo: &Topo, id: FaceId) -> Result<(), ValidationError> {
    for &l in &topo.faces[id].loops {
        let count = topo
            .faces
            .iter()
            .map(|(_, f)| f.loops.iter().filter(|&&x| x == l).count())
            .sum();
        once(l.into(), count)?;
    }
    Ok(())
}

/// Faces of a shell.
pub(super) fn shell_faces(topo: &Topo, id: ShellId) -> Result<(), ValidationError> {
    for u in &topo.shells[id].faces {
        let count = topo
            .shells
            .iter()
            .map(|(_, s)| s.faces.iter().filter(|x| x.face == u.face).count())
            .sum();
        once(u.face.into(), count)?;
    }
    Ok(())
}

/// Shells of a lump.
pub(super) fn lump_shells(topo: &Topo, id: LumpId) -> Result<(), ValidationError> {
    for &s in &topo.lumps[id].shells {
        let count = topo
            .lumps
            .iter()
            .map(|(_, l)| l.shells.iter().filter(|&&x| x == s).count())
            .sum();
        once(s.into(), count)?;
    }
    Ok(())
}

/// Lumps of a body.
pub(super) fn body_lumps(topo: &Topo, id: BodyId) -> Result<(), ValidationError> {
    for &l in &topo.bodies[id].lumps {
        let count = topo
            .bodies
            .iter()
            .map(|(_, b)| b.lumps.iter().filter(|&&x| x == l).count())
            .sum();
        once(l.into(), count)?;
    }
    Ok(())
}
