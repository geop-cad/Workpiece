//! A grid of cells around a solid, each known to be clear of the
//! solid's boundary — and then all inside it or all outside — or not
//! known to be. Many points are placed against a solid by it: a point
//! in a clear cell is where the cell is, and a point in another is told
//! from a clear cell nearby by what a short stretch to it crosses.
//!
//! No cell is called clear that a face comes into. Every Bézier patch
//! of every face is sampled so closely that none of its points is
//! farther than a cell's width from a sample (by a bound on how fast
//! the patch moves with its parameters, read off its control points),
//! and every cell within that of a sample, and the nearness asked for
//! besides, is taken to be on the boundary.

use shapecraft_core::math::Point3;

use crate::bezier::{H, Piece};
use crate::query::SolidData;

/// What is known of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    /// Nothing: a face may come into it.
    Unclear,
    /// It is clear of the boundary, and not yet told in from out.
    Clear,
    /// All of it is outside the solid.
    Out,
    /// All of it is inside the solid.
    In,
}

/// The cells around a solid.
#[derive(Debug, Clone)]
pub(crate) struct Grid {
    /// The low corner of the first cell.
    lo: Point3,
    /// A cell's width, the same along every axis.
    h: f64,
    /// How many cells along each axis.
    n: [usize; 3],
    cells: Vec<Cell>,
}

/// How many cells along the solid's longest side.
const ALONG: usize = 40;
/// More samples than this are not taken: the grid is made coarser.
const SAMPLES: f64 = 250_000.0;

/// How far a point of `patch` moves at most as each parameter goes
/// from 0 to 1; `None` for a patch with a weight that is not positive.
fn speeds(patch: &Piece) -> Option<[f64; 2]> {
    let (mut least, mut most) = (f64::INFINITY, 0.0_f64);
    for h in &patch.cp {
        least = least.min(h[3]);
        most = most.max(h[3]);
    }
    if least.is_nan() || least <= 0.0 || !most.is_finite() {
        return None;
    }
    let [nu, nv] = patch.n;
    let (mut du, mut dv) = (0.0_f64, 0.0_f64);
    for i in 0..nu {
        for j in 0..nv {
            let p = patch.at(i, j);
            if i + 1 < nu {
                du = du.max(p.distance(patch.at(i + 1, j)));
            }
            if j + 1 < nv {
                dv = dv.max(p.distance(patch.at(i, j + 1)));
            }
        }
    }
    // (A rational piece of degree n moves no faster than n times its
    // longest control leg times the square of how its weights spread.)
    let spread = (most / least).powi(2);
    Some([
        du * (nu.max(1) - 1) as f64 * spread,
        dv * (nv.max(1) - 1) as f64 * spread,
    ])
}

/// From `a` towards `b`, the part `t` of the way.
fn mix(a: &H, b: &H, t: f64) -> H {
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}

/// The point of the Bézier curve of homogeneous points `row` at `t`.
fn along(row: &mut [H], scratch: &[H], t: f64) -> Point3 {
    row.copy_from_slice(scratch);
    let n = row.len();
    for r in 1..n {
        for k in 0..n - r {
            row[k] = mix(&row[k], &row[k + 1], t);
        }
    }
    Point3::new(row[0][0], row[0][1], row[0][2]) / row[0][3]
}

impl Grid {
    /// The cells around the solid of `data`, those clear of its
    /// boundary by more than `pad` told in from out by `inside` (asked
    /// of one point of each stretch of clear cells that hang
    /// together). `None` if no grid can be laid.
    pub(crate) fn new(
        data: &SolidData,
        pad: f64,
        inside: &dyn Fn(Point3) -> Option<bool>,
    ) -> Option<Self> {
        let size = data.bounds.hi - data.bounds.lo;
        let longest = size.max_element();
        if longest.is_nan() || longest <= 0.0 || !longest.is_finite() || pad.is_nan() || pad < 0.0 {
            return None;
        }
        // (A patch with weights far apart is bounded loosely: it is
        // quartered, which brings the weights of each part together,
        // until it is not.)
        let mut patches: Vec<(Piece, [f64; 2])> = Vec::new();
        let mut open: Vec<(Piece, usize)> = data
            .faces
            .iter()
            .flat_map(|f| &f.patches)
            .map(|p| (p.clone(), 0))
            .collect();
        while let Some((piece, depth)) = open.pop() {
            let (mut least, mut most) = (f64::INFINITY, 0.0_f64);
            for h in &piece.cp {
                least = least.min(h[3]);
                most = most.max(h[3]);
            }
            if depth < 3 && least > 0.0 && most / least > 1.04 {
                for half in piece.halves(0) {
                    open.extend(half.halves(1).map(|quarter| (quarter, depth + 1)));
                }
                continue;
            }
            let speed = speeds(&piece)?;
            patches.push((piece, speed));
        }
        // As fine as the samples it takes allow.
        let mut count = ALONG;
        let h = loop {
            let h = longest / count as f64;
            let samples: f64 = patches
                .iter()
                .map(|(_, [u, v])| (u / h).ceil().max(1.0) * (v / h).ceil().max(1.0))
                .sum();
            if samples <= SAMPLES {
                break h;
            }
            count /= 2;
            if count < 8 {
                return None;
            }
        };
        if pad > h {
            return None;
        }
        // A cell more on every side: outside the solid's box.
        let lo = data.bounds.lo - Point3::splat(h);
        let n = [0, 1, 2].map(|k| (size[k] / h).ceil() as usize + 2);
        let mut cells = vec![Cell::Clear; n[0] * n[1] * n[2]];
        let index = |i: usize, j: usize, k: usize| (i * n[1] + j) * n[2] + k;
        // Every cell a face may come into: the cell of each sample (and
        // those within the nearness of it, where it is that close to
        // the cell's side), and then every cell beside one of those —
        // which is every cell within a cell's width and the nearness
        // of a sample, and a few more.
        let near = pad + longest * 1e-12;
        let mut hit = vec![false; cells.len()];
        let (mut row, mut scratch): (Vec<H>, Vec<H>) = (Vec::new(), Vec::new());
        let mut line: Vec<H> = Vec::new();
        let inv = 1.0 / h;
        let top = [n[0] - 1, n[1] - 1, n[2] - 1];
        for (done, (patch, [su, sv])) in patches.iter().enumerate() {
            if done % 64 == 0 {
                shapecraft_core::cancel::checkpoint();
            }
            let [nu, nv] = patch.n;
            // (Half a step to each side of a sample is within a cell's
            // width in all: half of it along each parameter.)
            let ku = (su / h).ceil().max(1.0) as usize;
            let kv = (sv / h).ceil().max(1.0) as usize;
            scratch.resize(nu, [0.0; 4]);
            row.resize(nu, [0.0; 4]);
            line.resize(nv, [0.0; 4]);
            for b in 0..kv {
                let v = (b as f64 + 0.5) / kv as f64;
                // The patch's curve along u at this v.
                for (i, at) in scratch.iter_mut().enumerate() {
                    line.copy_from_slice(&patch.cp[i * nv..(i + 1) * nv]);
                    for r in 1..nv {
                        for k in 0..nv - r {
                            line[k] = mix(&line[k], &line[k + 1], v);
                        }
                    }
                    *at = line[0];
                }
                for a in 0..ku {
                    let p = along(&mut row, &scratch, (a as f64 + 0.5) / ku as f64);
                    let at = (p - lo) * inv;
                    // The cells along each axis the sample is in or
                    // within the nearness of: one, or two.
                    let mut span = [(0usize, 0usize); 3];
                    for k in 0..3 {
                        let (low, high) = (at[k] - near * inv, at[k] + near * inv);
                        span[k] = (
                            (low.max(0.0) as usize).min(top[k]),
                            (high.max(0.0) as usize).min(top[k]),
                        );
                    }
                    for i in span[0].0..=span[0].1 {
                        for j in span[1].0..=span[1].1 {
                            for k in span[2].0..=span[2].1 {
                                hit[index(i, j, k)] = true;
                            }
                        }
                    }
                }
            }
        }
        // Those, and the cells beside them: along each axis in turn.
        let (ni, nj, nk) = (n[0], n[1], n[2]);
        let mut wide = hit.clone();
        for (axis, step) in [(2, 1), (1, nk), (0, nj * nk)] {
            let count = [ni, nj, nk][axis];
            for at in 0..hit.len() {
                if !hit[at] {
                    continue;
                }
                let along = match axis {
                    0 => at / (nj * nk),
                    1 => at / nk % nj,
                    _ => at % nk,
                };
                if along > 0 {
                    wide[at - step] = true;
                }
                if along + 1 < count {
                    wide[at + step] = true;
                }
            }
            hit.copy_from_slice(&wide);
        }
        for (cell, hit) in cells.iter_mut().zip(&hit) {
            if *hit {
                *cell = Cell::Unclear;
            }
        }
        // The clear cells, stretch by stretch (cells that hang together
        // side to side): what reaches the rim of the grid is outside
        // the solid's box, and any other stretch is where its first
        // cell's middle is.
        let mut stack: Vec<usize> = Vec::new();
        let mut spread = |cells: &mut Vec<Cell>, start: usize, to: Cell, seen: &mut Vec<usize>| {
            cells[start] = to;
            stack.push(start);
            while let Some(at) = stack.pop() {
                seen.push(at);
                let (i, j, k) = (at / (nj * nk), at / nk % nj, at % nk);
                let mut visit = |next: usize| {
                    if cells[next] == Cell::Clear {
                        cells[next] = to;
                        stack.push(next);
                    }
                };
                if i > 0 {
                    visit(at - nj * nk);
                }
                if i + 1 < ni {
                    visit(at + nj * nk);
                }
                if j > 0 {
                    visit(at - nk);
                }
                if j + 1 < nj {
                    visit(at + nk);
                }
                if k > 0 {
                    visit(at - 1);
                }
                if k + 1 < nk {
                    visit(at + 1);
                }
            }
        };
        let mut seen = Vec::new();
        for i in 0..ni {
            for j in 0..nj {
                let rim = i == 0 || j == 0 || i + 1 == ni || j + 1 == nj;
                let step = if rim { 1 } else { (nk - 1).max(1) };
                for k in (0..nk).step_by(step) {
                    let at = index(i, j, k);
                    if cells[at] == Cell::Clear {
                        spread(&mut cells, at, Cell::Out, &mut seen);
                        seen.clear();
                    }
                }
            }
        }
        for start in 0..cells.len() {
            if cells[start] != Cell::Clear {
                continue;
            }
            // (Held as unclear until it is told.)
            spread(&mut cells, start, Cell::Unclear, &mut seen);
            let (i, j, k) = (start / (nj * nk), start / nk % nj, start % nk);
            let middle = lo + Point3::new(i as f64 + 0.5, j as f64 + 0.5, k as f64 + 0.5) * h;
            let cell = match inside(middle) {
                Some(true) => Some(Cell::In),
                Some(false) => Some(Cell::Out),
                None => None,
            };
            for at in seen.drain(..) {
                if let Some(cell) = cell {
                    cells[at] = cell;
                }
            }
        }
        Some(Self { lo, h, n, cells })
    }

    /// The cell `p` is in, if it is in the grid.
    fn cell(&self, p: Point3) -> Option<[usize; 3]> {
        let at = ((p - self.lo) / self.h).floor();
        let mut cell = [0; 3];
        for k in 0..3 {
            if at[k].is_nan() || at[k] < 0.0 || at[k] >= self.n[k] as f64 {
                return None;
            }
            cell[k] = at[k] as usize;
        }
        Some(cell)
    }

    fn at(&self, [i, j, k]: [usize; 3]) -> Cell {
        self.cells[(i * self.n[1] + j) * self.n[2] + k]
    }

    /// Whether `p` is inside the solid, if it is in a cell clear of the
    /// boundary.
    pub(crate) fn holds(&self, p: Point3) -> Option<bool> {
        match self.at(self.cell(p)?) {
            Cell::In => Some(true),
            Cell::Out => Some(false),
            _ => None,
        }
    }

    /// The middles of the clear cells nearest `p`, the nearest first,
    /// each with whether it is inside the solid: up to three of them,
    /// none more than two cells off the cell `p` is in.
    pub(crate) fn clear_near(&self, p: Point3) -> impl Iterator<Item = (Point3, bool)> {
        let mut found = [(f64::INFINITY, Point3::ZERO, false); 3];
        if let Some(cell) = self.cell(p) {
            let range = |k: usize| cell[k].saturating_sub(2)..=(cell[k] + 2).min(self.n[k] - 1);
            for i in range(0) {
                for j in range(1) {
                    for k in range(2) {
                        let inside = match self.at([i, j, k]) {
                            Cell::In => true,
                            Cell::Out => false,
                            _ => continue,
                        };
                        let middle = self.lo
                            + Point3::new(i as f64 + 0.5, j as f64 + 0.5, k as f64 + 0.5) * self.h;
                        let mut next = (middle.distance_squared(p), middle, inside);
                        for slot in &mut found {
                            if next.0 < slot.0 {
                                std::mem::swap(slot, &mut next);
                            }
                        }
                    }
                }
            }
        }
        found
            .into_iter()
            .filter(|f| f.0.is_finite())
            .map(|(_, p, inside)| (p, inside))
    }
}
