//! The tree of boxes that finds what a ray hits without trying every
//! triangle: each node's box holds all triangles below it, and a ray
//! that misses the box skips them all.

use glam::Vec3A;

use crate::Triangle;

/// How many bins the triangles of a node are sorted into to find a
/// good place to split it.
const BINS: usize = 16;
/// The most triangles a leaf holds.
const LEAF: usize = 4;
/// From this depth on, nodes are split in halves, which bounds the
/// depth of the tree.
const DEEP: u32 = 40;
/// The most nodes that can wait while a ray walks the tree.
const STACK: usize = 96;
/// How far outside its edges a triangle still counts as hit, so that
/// no ray slips between two triangles that share an edge.
const EDGE: f32 = 1e-6;

/// Where a ray met a triangle.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hit {
    /// The distance along the ray.
    pub t: f32,
    /// Which triangle, as the tree numbers them.
    pub slot: u32,
    /// The weight of the triangle's second corner at the hit.
    pub u: f32,
    /// The weight of the triangle's third corner at the hit.
    pub v: f32,
}

/// A node of the tree. A leaf (`count > 0`) holds the triangles from
/// `at` on; any other node has its two children at `at` and `at + 1`.
#[derive(Clone, Copy)]
struct Node {
    min: [f32; 3],
    at: u32,
    max: [f32; 3],
    count: u32,
}

/// A triangle while the tree is built: its box and its number.
#[derive(Clone, Copy)]
struct Prim {
    min: [f32; 3],
    id: u32,
    max: [f32; 3],
}

impl Prim {
    /// Twice the middle of the box; where the triangle is sorted by.
    fn middle(&self) -> Vec3A {
        Vec3A::from_array(self.min) + Vec3A::from_array(self.max)
    }

    fn bounds(&self) -> Bounds {
        Bounds {
            min: Vec3A::from_array(self.min),
            max: Vec3A::from_array(self.max),
        }
    }
}

/// A box along the axes.
#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3A,
    max: Vec3A,
}

impl Bounds {
    const EMPTY: Self = Self {
        min: Vec3A::INFINITY,
        max: Vec3A::NEG_INFINITY,
    };

    fn add(&mut self, other: &Bounds) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    fn add_point(&mut self, point: Vec3A) {
        self.min = self.min.min(point);
        self.max = self.max.max(point);
    }

    /// Half the area of the box's faces; zero for an empty box.
    fn area(&self) -> f32 {
        let d = self.max - self.min;
        if d.x < 0.0 {
            0.0
        } else {
            d.x * d.y + d.y * d.z + d.z * d.x
        }
    }

    fn leaf(&self, first: usize, count: usize) -> Node {
        Node {
            min: self.min.to_array(),
            at: first as u32,
            max: self.max.to_array(),
            count: count as u32,
        }
    }
}

/// The triangles of a scene, sorted into a tree of boxes.
pub(crate) struct Bvh {
    nodes: Vec<Node>,
    /// A corner and the two edges from it, of every triangle, in the
    /// order of the tree.
    tris: Vec<[Vec3A; 3]>,
    /// The number each triangle has in the scene.
    order: Vec<u32>,
}

impl Bvh {
    /// Sorts the triangles into a tree, and tells the box around all
    /// of them. Triangles with a corner that is not a number are left
    /// out.
    pub(crate) fn build(triangles: &[Triangle]) -> (Self, Option<(Vec3A, Vec3A)>) {
        let mut prims = Vec::with_capacity(triangles.len());
        let mut all = Bounds::EMPTY;
        for (id, triangle) in triangles.iter().enumerate() {
            let [a, b, c] = triangle.positions.map(Vec3A::from);
            if !(a.is_finite() && b.is_finite() && c.is_finite()) {
                continue;
            }
            let bounds = Bounds {
                min: a.min(b).min(c),
                max: a.max(b).max(c),
            };
            all.add(&bounds);
            prims.push(Prim {
                min: bounds.min.to_array(),
                id: id as u32,
                max: bounds.max.to_array(),
            });
        }
        if prims.is_empty() {
            let empty = Self {
                nodes: Vec::new(),
                tris: Vec::new(),
                order: Vec::new(),
            };
            return (empty, None);
        }

        let mut nodes = Vec::with_capacity(prims.len() / 2 + 16);
        nodes.push(all.leaf(0, prims.len()));
        let mut waiting = vec![(0usize, 0usize, prims.len(), all.area(), 0u32)];
        while let Some((node, first, count, area, depth)) = waiting.pop() {
            let Some((mid, left, right)) = split(&mut prims[first..first + count], area, depth)
            else {
                continue;
            };
            let children = nodes.len();
            nodes.push(left.leaf(first, mid));
            nodes.push(right.leaf(first + mid, count - mid));
            nodes[node].at = children as u32;
            nodes[node].count = 0;
            waiting.push((children, first, mid, left.area(), depth + 1));
            waiting.push((
                children + 1,
                first + mid,
                count - mid,
                right.area(),
                depth + 1,
            ));
        }

        let tris = prims
            .iter()
            .map(|prim| {
                let [a, b, c] = triangles[prim.id as usize].positions.map(Vec3A::from);
                [a, b - a, c - a]
            })
            .collect();
        let order = prims.iter().map(|prim| prim.id).collect();
        (Self { nodes, tris, order }, Some((all.min, all.max)))
    }

    /// A corner of a triangle and the two edges from it.
    pub(crate) fn corner(&self, slot: u32) -> [Vec3A; 3] {
        self.tris[slot as usize]
    }

    /// The number a triangle of the tree has in the scene.
    pub(crate) fn id(&self, slot: u32) -> usize {
        self.order[slot as usize] as usize
    }

    /// The nearest triangle a ray meets before `reach`.
    pub(crate) fn nearest(&self, origin: Vec3A, dir: Vec3A, reach: f32) -> Option<Hit> {
        let root = self.nodes.first()?;
        let inv = inverse(dir);
        let mut best = Hit {
            t: reach,
            slot: u32::MAX,
            u: 0.0,
            v: 0.0,
        };
        if enter(root, origin, inv, best.t) == f32::INFINITY {
            return None;
        }
        let mut stack = [(0u32, 0f32); STACK];
        let mut top = 0;
        let mut node = root;
        loop {
            if node.count > 0 {
                for slot in node.at..node.at + node.count {
                    if let Some((t, u, v)) = meet(&self.tris[slot as usize], origin, dir)
                        && t < best.t
                    {
                        best = Hit { t, slot, u, v };
                    }
                }
            } else {
                let (a, b) = (node.at, node.at + 1);
                let ta = enter(&self.nodes[a as usize], origin, inv, best.t);
                let tb = enter(&self.nodes[b as usize], origin, inv, best.t);
                let (near, far, t_near, t_far) = if ta <= tb {
                    (a, b, ta, tb)
                } else {
                    (b, a, tb, ta)
                };
                if t_near < f32::INFINITY {
                    if t_far < f32::INFINITY && top < STACK {
                        stack[top] = (far, t_far);
                        top += 1;
                    }
                    node = &self.nodes[near as usize];
                    continue;
                }
            }
            loop {
                if top == 0 {
                    return (best.slot != u32::MAX).then_some(best);
                }
                top -= 1;
                let (next, t) = stack[top];
                if t < best.t {
                    node = &self.nodes[next as usize];
                    break;
                }
            }
        }
    }

    /// Calls `seen` with every triangle a ray meets before `reach`, in
    /// no order, until it answers `false`.
    pub(crate) fn each(
        &self,
        origin: Vec3A,
        dir: Vec3A,
        reach: f32,
        mut seen: impl FnMut(u32) -> bool,
    ) {
        if self.nodes.is_empty() {
            return;
        }
        let inv = inverse(dir);
        let mut stack = [0u32; STACK];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let node = &self.nodes[stack[top] as usize];
            if enter(node, origin, inv, reach) == f32::INFINITY {
                continue;
            }
            if node.count > 0 {
                for slot in node.at..node.at + node.count {
                    if let Some((t, _, _)) = meet(&self.tris[slot as usize], origin, dir)
                        && t < reach
                        && !seen(slot)
                    {
                        return;
                    }
                }
            } else if top + 2 <= STACK {
                stack[top] = node.at;
                stack[top + 1] = node.at + 1;
                top += 2;
            }
        }
    }

    /// The nearest triangle a ray meets, found by trying every one.
    #[cfg(test)]
    pub(crate) fn nearest_slow(&self, origin: Vec3A, dir: Vec3A) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for (slot, tri) in self.tris.iter().enumerate() {
            if let Some((t, u, v)) = meet(tri, origin, dir)
                && best.is_none_or(|b| t < b.t)
            {
                best = Some(Hit {
                    t,
                    slot: slot as u32,
                    u,
                    v,
                });
            }
        }
        best
    }
}

/// One over each part of a direction, kept finite so that a ray along
/// a face of a box is still found inside it.
fn inverse(dir: Vec3A) -> Vec3A {
    dir.recip().clamp(Vec3A::splat(-1e30), Vec3A::splat(1e30))
}

/// The distance at which a ray enters a node's box, or infinity when
/// it misses the box or enters it only beyond `reach`.
#[inline(always)]
fn enter(node: &Node, origin: Vec3A, inv: Vec3A, reach: f32) -> f32 {
    let a = (Vec3A::from_array(node.min) - origin) * inv;
    let b = (Vec3A::from_array(node.max) - origin) * inv;
    let near = a.min(b).max_element().max(0.0);
    let far = a.max(b).min_element();
    if near <= far * 1.000_01 && near < reach {
        near
    } else {
        f32::INFINITY
    }
}

/// Where a ray meets a triangle (given as a corner and two edges),
/// from either side: the distance and the weights of the second and
/// third corner.
#[inline(always)]
fn meet(tri: &[Vec3A; 3], origin: Vec3A, dir: Vec3A) -> Option<(f32, f32, f32)> {
    let [corner, e1, e2] = *tri;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det == 0.0 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - corner;
    let u = s.dot(p) * inv;
    if !(-EDGE..=1.0 + EDGE).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < -EDGE || u + v > 1.0 + EDGE {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0 && t.is_finite()).then_some((t, u, v))
}

/// Sorts the triangles of a node into a left and a right part, and
/// tells how many went left and the boxes of both parts; or `None`
/// when the node is better left whole. `area` is that of the node's
/// box.
fn split(prims: &mut [Prim], area: f32, depth: u32) -> Option<(usize, Bounds, Bounds)> {
    let count = prims.len();
    if count <= 2 {
        return None;
    }
    let mut middles = Bounds::EMPTY;
    for prim in prims.iter() {
        middles.add_point(prim.middle());
    }
    let extent = middles.max - middles.min;
    let widest = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    if depth >= DEEP || extent[widest] <= 0.0 {
        return (count > LEAF).then(|| halve(prims, widest));
    }

    // Sort the triangles into bins along each axis, then try a cut
    // between every two bins and keep the cheapest: a part costs its
    // box's area (the chance a ray enters it) times its triangles.
    let scale = Vec3A::select(
        extent.cmpgt(Vec3A::ZERO),
        Vec3A::splat(BINS as f32) / extent,
        Vec3A::ZERO,
    );
    let bin = |prim: &Prim| {
        ((prim.middle() - middles.min) * scale)
            .to_array()
            .map(|x| (x as usize).min(BINS - 1))
    };
    let mut bins = [[(Bounds::EMPTY, 0u32); BINS]; 3];
    for prim in prims.iter() {
        let at = bin(prim);
        let bounds = prim.bounds();
        for axis in 0..3 {
            let slot = &mut bins[axis][at[axis]];
            slot.0.add(&bounds);
            slot.1 += 1;
        }
    }
    let mut best = (f32::INFINITY, 0, 0);
    for axis in 0..3 {
        if extent[axis] <= 0.0 {
            continue;
        }
        let mut right = [0.0; BINS];
        let mut acc = (Bounds::EMPTY, 0u32);
        for i in (1..BINS).rev() {
            acc.0.add(&bins[axis][i].0);
            acc.1 += bins[axis][i].1;
            right[i] = acc.0.area() * acc.1 as f32;
        }
        let mut acc = (Bounds::EMPTY, 0u32);
        for i in 0..BINS - 1 {
            acc.0.add(&bins[axis][i].0);
            acc.1 += bins[axis][i].1;
            if acc.1 == 0 || acc.1 as usize == count {
                continue;
            }
            let cost = acc.0.area() * acc.1 as f32 + right[i + 1];
            if cost < best.0 {
                best = (cost, axis, i);
            }
        }
    }
    let (cost, axis, cut) = best;
    if cost == f32::INFINITY {
        return (count > LEAF).then(|| halve(prims, widest));
    }
    if count <= LEAF && cost + area >= count as f32 * area {
        return None;
    }

    let mut mid = 0;
    let mut end = count;
    while mid < end {
        if bin(&prims[mid])[axis] <= cut {
            mid += 1;
        } else {
            end -= 1;
            prims.swap(mid, end);
        }
    }
    if mid == 0 || mid == count {
        return Some(halve(prims, widest));
    }
    let mut left = Bounds::EMPTY;
    let mut right = Bounds::EMPTY;
    for (i, slot) in bins[axis].iter().enumerate() {
        if i <= cut {
            left.add(&slot.0);
        } else {
            right.add(&slot.0);
        }
    }
    Some((mid, left, right))
}

/// Splits the triangles of a node into two halves along an axis.
fn halve(prims: &mut [Prim], axis: usize) -> (usize, Bounds, Bounds) {
    let mid = prims.len() / 2;
    prims.select_nth_unstable_by(mid, |a, b| a.middle()[axis].total_cmp(&b.middle()[axis]));
    let around = |part: &[Prim]| {
        let mut bounds = Bounds::EMPTY;
        for prim in part {
            bounds.add(&prim.bounds());
        }
        bounds
    };
    (mid, around(&prims[..mid]), around(&prims[mid..]))
}
