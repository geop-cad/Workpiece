//! The buffers as interleaved `f32` vertex data for the GPU, six vertices
//! (two triangles) per line and per vertex marker.
//!
//! Lines and markers are quads whose corners the vertex shader moves out
//! in screen space: each vertex carries the primitive's endpoints and a
//! corner code, so the quad always faces the camera and keeps its width in
//! pixels. Every vertex ends with its [`PickId`](crate::PickId) as a float
//! (`-1` for none), for highlighting.

use crate::{Line, Triangle, Vertex};

/// Floats per mesh vertex: position (3), normal (3), color (4), pick
/// (1), texture (2: its number and the length of a repeat).
pub const MESH_STRIDE: usize = 13;
/// Floats per line vertex: start (3), end (3), corner (2: `t` along, 0 at
/// the start and 1 at the end; `s` across, −1 or 1), widths at start and
/// end (2), color (4), pick (1).
pub const LINE_STRIDE: usize = 15;
/// Floats per marker vertex: center (3), corner (2, each −1 or 1), size
/// (1), color (4), pick (1).
pub const VERTEX_STRIDE: usize = 11;

/// The corners of a quad's two triangles: (along, across).
const LINE_CORNERS: [[f32; 2]; 6] = [
    [0.0, -1.0],
    [1.0, -1.0],
    [0.0, 1.0],
    [0.0, 1.0],
    [1.0, -1.0],
    [1.0, 1.0],
];
const VERTEX_CORNERS: [[f32; 2]; 6] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [-1.0, 1.0],
    [-1.0, 1.0],
    [1.0, -1.0],
    [1.0, 1.0],
];

fn pick(id: Option<u32>) -> f32 {
    id.map_or(-1.0, |id| id as f32)
}

/// The vertex data of the opaque (`translucent == false`) or translucent
/// triangles.
pub fn mesh<'a>(triangles: impl IntoIterator<Item = &'a Triangle>, translucent: bool) -> Vec<f32> {
    let mut out = Vec::new();
    for t in triangles {
        if (t.color[3] < 1.0) != translucent {
            continue;
        }
        for k in 0..3 {
            out.extend(t.positions[k].to_array());
            out.extend(t.normals[k].to_array());
            out.extend(t.color);
            out.push(pick(t.pick));
            out.extend(t.texture);
        }
    }
    out
}

/// The vertex data of `lines`.
pub fn lines<'a>(lines: impl IntoIterator<Item = &'a Line>) -> Vec<f32> {
    let mut out = Vec::new();
    for l in lines {
        for corner in LINE_CORNERS {
            out.extend(l.a.to_array());
            out.extend(l.b.to_array());
            out.extend(corner);
            out.extend(l.width);
            out.extend(l.color);
            out.push(pick(l.pick));
        }
    }
    out
}

/// The vertex data of `vertices`.
pub fn vertices<'a>(vertices: impl IntoIterator<Item = &'a Vertex>) -> Vec<f32> {
    let mut out = Vec::new();
    for v in vertices {
        for corner in VERTEX_CORNERS {
            out.extend(v.position.to_array());
            out.extend(corner);
            out.push(v.size);
            out.extend(v.color);
            out.push(pick(v.pick));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn primitives_expand_to_whole_vertices() {
        let line = Line {
            a: Vec3::ZERO,
            b: Vec3::X,
            width: [2.0, 0.0],
            color: [1.0; 4],
            pick: Some(3),
        };
        let data = lines([&line]);
        assert_eq!(data.len(), 6 * LINE_STRIDE);
        assert_eq!(data[LINE_STRIDE - 1], 3.0);
        let t = Triangle {
            positions: [Vec3::ZERO, Vec3::X, Vec3::Y],
            normals: [Vec3::Z; 3],
            color: [1.0, 1.0, 1.0, 0.5],
            pick: None,
            texture: [0.0, 1.0],
        };
        assert!(mesh([&t], false).is_empty());
        assert_eq!(mesh([&t], true).len(), 3 * MESH_STRIDE);
    }
}
