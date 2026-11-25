//! Shells: non-empty and connected through shared edges.

use std::collections::BTreeMap;

use super::ValidationError;
use crate::world::topo::{ShellId, Topo};

/// A shell has faces, and every face is reachable from the first one by
/// crossing edges.
pub(super) fn check(topo: &Topo, id: ShellId) -> Result<(), ValidationError> {
    let faces = &topo.shells[id].faces;
    if faces.is_empty() {
        return Err(ValidationError::EmptyShell(id));
    }

    // Union-find over face positions, joined through shared edges.
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut first_face_of_edge = BTreeMap::new();
    for (i, u) in faces.iter().enumerate() {
        for &l in &topo.faces[u.face].loops {
            for &c in &topo.loops[l].coedges {
                let Some(e) = topo.coedges[c].edge_id() else {
                    continue; // poles join nothing
                };
                let j = *first_face_of_edge.entry(e).or_insert(i);
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let r = root(&mut parent, 0);
    if (0..faces.len()).all(|i| root(&mut parent, i) == r) {
        Ok(())
    } else {
        Err(ValidationError::DisconnectedShell(id))
    }
}
