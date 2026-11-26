//! Fill patterns: the places of a grid that fit in a face — a square or
//! rectangular grid, staggered rows, a hexagonal packing, concentric
//! rings, or rings of a polygon — laid out from where what is repeated
//! is, and kept only where the whole of it stays inside the face, a
//! margin in from its edges.

use anyhow::{Context, ensure};
use workpiece_core::math::{Point2, Point3};
use workpiece_core::world::DatumPlane;
use workpiece_core::world::topo::{FaceId, Orientation, Topo};

/// How a face is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Grid {
    /// Rows and columns the same spacing apart.
    #[default]
    Square,
    /// Rows one spacing apart, columns another.
    Rectangular,
    /// Rows, every other one shifted by half a spacing: a perforation.
    Staggered,
    /// Each a spacing from its six neighbours: the closest packing.
    Hexagonal,
    /// Rings around the first, each a ring spacing further out, the
    /// places along each a spacing apart.
    Rings,
    /// Rings of a polygon around the first, its sides a spacing apart.
    Polygon,
}

impl workpiece_core::Choices for Grid {
    const ALL: &'static [Self] = &[
        Grid::Square,
        Grid::Rectangular,
        Grid::Staggered,
        Grid::Hexagonal,
        Grid::Rings,
        Grid::Polygon,
    ];

    fn label(self) -> &'static str {
        match self {
            Grid::Square => "Square",
            Grid::Rectangular => "Rectangular",
            Grid::Staggered => "Staggered",
            Grid::Hexagonal => "Hexagonal",
            Grid::Rings => "Rings",
            Grid::Polygon => "Polygon",
        }
    }

    fn icon(self) -> Option<&'static str> {
        Some(match self {
            Grid::Square => "fill-square",
            Grid::Rectangular => "fill-rectangular",
            Grid::Staggered => "fill-staggered",
            Grid::Hexagonal => "fill-hexagonal",
            Grid::Rings => "fill-rings",
            Grid::Polygon => "fill-polygon",
        })
    }
}

/// The sizes of a fill.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    /// How it is filled.
    pub grid: Grid,
    /// How far apart the places are along a row (or a ring).
    pub spacing: f64,
    /// How far apart the rows (or the rings) are, where that is a size
    /// of its own: of a rectangular or staggered grid, of rings.
    pub across: f64,
    /// Which way the rows run, in radians from the plane's x direction.
    pub angle: f64,
    /// How many sides the polygon has.
    pub sides: usize,
    /// How far in from the face's edges everything stays.
    pub margin: f64,
    /// How far what is repeated reaches from its place: that far more
    /// it stays in from the edges.
    pub reach: f64,
}

/// The most places a fill makes.
const MOST: usize = 20_000;

/// The places of `layout` from `seed` (where what is repeated is, in
/// the face's plane) that fit in the region `outline` bounds (closed
/// polygons: the outer edge and those of holes, in any order): those
/// at least the margin and the reach in from every edge. The seed
/// itself is not among them.
pub fn places(
    layout: &Layout,
    outline: &[Vec<Point2>],
    seed: Point2,
) -> anyhow::Result<Vec<Point2>> {
    let Layout {
        grid,
        spacing,
        across,
        angle,
        sides,
        margin,
        reach,
    } = *layout;
    ensure!(
        spacing > 0.0 && spacing.is_finite(),
        "the places are some way apart"
    );
    let across = match grid {
        Grid::Square => spacing,
        Grid::Hexagonal => spacing * 3f64.sqrt() / 2.0,
        Grid::Rectangular | Grid::Staggered | Grid::Rings | Grid::Polygon => across,
    };
    ensure!(
        across > 0.0 && across.is_finite(),
        "the rows are some way apart"
    );
    ensure!(margin >= 0.0, "a margin is not less than nothing");
    let points: Vec<Point2> = outline.iter().flatten().copied().collect();
    ensure!(!points.is_empty(), "the face has no edges to fill within");
    // As far as the face reaches from the seed: no place further out
    // is in it.
    let far = points.iter().map(|p| p.distance(seed)).fold(0.0, f64::max);
    let (u, v) = (
        Point2::new(libm::cos(angle), libm::sin(angle)),
        Point2::new(-libm::sin(angle), libm::cos(angle)),
    );
    let mut all = Vec::new();
    let mut add = |p: Point2| -> anyhow::Result<()> {
        ensure!(
            all.len() < MOST * 4,
            "the spacing is too small for this face: over {MOST} places"
        );
        all.push(p);
        Ok(())
    };
    match grid {
        Grid::Square | Grid::Rectangular | Grid::Staggered | Grid::Hexagonal => {
            let staggered = matches!(grid, Grid::Staggered | Grid::Hexagonal);
            let rows = (far / across).ceil() as i64 + 1;
            let columns = (far / spacing).ceil() as i64 + 1;
            for j in -rows..=rows {
                let shift = if staggered && j.rem_euclid(2) == 1 {
                    0.5
                } else {
                    0.0
                };
                for i in -columns..=columns {
                    add(seed + u * ((i as f64 + shift) * spacing) + v * (j as f64 * across))?;
                }
            }
        }
        Grid::Rings => {
            let rings = (far / across).ceil() as usize + 1;
            for k in 1..=rings {
                let r = k as f64 * across;
                let n = ((std::f64::consts::TAU * r / spacing).floor() as usize).max(1);
                for i in 0..n {
                    let a = angle + std::f64::consts::TAU * i as f64 / n as f64;
                    add(seed + Point2::new(libm::cos(a), libm::sin(a)) * r)?;
                }
            }
        }
        Grid::Polygon => {
            ensure!(sides >= 3, "a polygon has three sides or more");
            let rings = (far / across).ceil() as usize + 1;
            let corner = |r: f64, c: usize| {
                let a = angle + std::f64::consts::TAU * c as f64 / sides as f64;
                seed + Point2::new(libm::cos(a), libm::sin(a)) * r
            };
            for k in 1..=rings {
                // (Its corners `across` further out each ring.)
                let r = k as f64 * across;
                for c in 0..sides {
                    let (a, b) = (corner(r, c), corner(r, c + 1));
                    let n = ((a.distance(b) / spacing).round() as usize).max(1);
                    for i in 0..n {
                        add(a + (b - a) * (i as f64 / n as f64))?;
                    }
                }
            }
        }
    }
    let within = margin + reach.max(0.0);
    let close = spacing.min(across) * 1e-6;
    let kept: Vec<Point2> = all
        .into_iter()
        .filter(|p| p.distance(seed) > close)
        .filter(|p| inside(outline, *p) && edge_distance(outline, *p) >= within)
        .collect();
    ensure!(
        kept.len() <= MOST,
        "the spacing is too small for this face: over {MOST} places"
    );
    Ok(kept)
}

/// Whether `p` is inside the region `outline` bounds: inside an odd
/// number of its polygons.
pub fn inside(outline: &[Vec<Point2>], p: Point2) -> bool {
    let mut odd = false;
    for polygon in outline {
        for (k, a) in polygon.iter().enumerate() {
            let b = polygon[(k + 1) % polygon.len()];
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
    }
    odd
}

/// How far `p` is from the nearest edge of `outline`.
fn edge_distance(outline: &[Vec<Point2>], p: Point2) -> f64 {
    let mut nearest = f64::INFINITY;
    for polygon in outline {
        for (k, a) in polygon.iter().enumerate() {
            let b = polygon[(k + 1) % polygon.len()];
            let ab = b - *a;
            let t = ((p - *a).dot(ab) / ab.length_squared().max(1e-300)).clamp(0.0, 1.0);
            nearest = nearest.min(p.distance(*a + ab * t));
        }
    }
    nearest
}

/// The edges of the face `face` of `topo`, as polygons in `plane`'s
/// own coordinates: each loop, its edges sampled.
pub fn outline(topo: &Topo, face: FaceId, plane: &DatumPlane) -> anyhow::Result<Vec<Vec<Point2>>> {
    const SAMPLES: usize = 24;
    let f = topo.face(face).context("the face is gone")?;
    let local = |p: Point3| {
        let d = p - plane.origin();
        Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
    };
    let mut loops = Vec::new();
    for &l in &f.loops {
        let mut polygon = Vec::new();
        for &c in &topo.loop_(l).context("a loop of the face")?.coedges {
            let coedge = topo.coedge(c).context("an edge of the face")?;
            let Some(edge) = coedge.edge_id().and_then(|e| topo.edge(e)) else {
                continue;
            };
            let curve = topo.curve(edge.curve).context("an edge's curve")?;
            let forward = !matches!(
                coedge.kind,
                workpiece_core::world::topo::CoedgeKind::Edge {
                    orientation: Orientation::Reversed,
                    ..
                }
            );
            for i in 0..SAMPLES {
                let s = i as f64 / SAMPLES as f64;
                let s = if forward { s } else { 1.0 - s };
                polygon.push(local(curve.point_at(edge.t0 + (edge.t1 - edge.t0) * s)));
            }
        }
        if polygon.len() >= 3 {
            loops.push(polygon);
        }
    }
    ensure!(!loops.is_empty(), "the face has no edges to fill within");
    Ok(loops)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square 100 across around the origin.
    fn square() -> Vec<Vec<Point2>> {
        vec![
            [(-50.0, -50.0), (50.0, -50.0), (50.0, 50.0), (-50.0, 50.0)]
                .map(|(x, y)| Point2::new(x, y))
                .to_vec(),
        ]
    }

    fn layout(grid: Grid) -> Layout {
        Layout {
            grid,
            spacing: 10.0,
            across: 10.0,
            angle: 0.0,
            sides: 6,
            margin: 0.0,
            reach: 2.0,
        }
    }

    #[test]
    fn each_kind_of_fill_counts_its_places_in_the_face() -> anyhow::Result<()> {
        let at = |grid| places(&layout(grid), &square(), Point2::ZERO).map(|p| p.len());
        // A square grid 10 apart: −40 … 40 both ways (50 is 2 short of
        // the edge for what reaches 2), the seed not counted.
        assert_eq!(at(Grid::Square)?, 9 * 9 - 1);
        // Rows 20 apart: −40, −20, …, 40.
        let mut rectangular = layout(Grid::Rectangular);
        rectangular.across = 20.0;
        assert_eq!(
            places(&rectangular, &square(), Point2::ZERO)?.len(),
            9 * 5 - 1
        );
        // Every other row shifted by 5: those rows have −45 … 45, ten.
        assert_eq!(at(Grid::Staggered)?, 5 * 9 + 4 * 10 - 1);
        // Hexagonal: rows 8.66 apart, j = −5 … 5 (5 × 8.66 = 43.3), the
        // odd ones shifted.
        assert_eq!(at(Grid::Hexagonal)?, 5 * 9 + 6 * 10 - 1);
        // Rings 10 apart: 6, 12, 18, 25 places on the first four; of
        // the fifth (r = 50) and sixth, only those well inside the
        // square's corners stay.
        let rings = at(Grid::Rings)?;
        assert!(rings > 6 + 12 + 18 + 25 && rings < 200, "{rings}");
        // Hexagons: 6 k on the k-th ring, the 4 first in all (corners at
        // 40), of the 5th and 6th only those inside.
        let hexagons = at(Grid::Polygon)?;
        assert!(hexagons >= 6 + 12 + 18 + 24, "{hexagons}");
        Ok(())
    }

    #[test]
    fn what_would_cross_the_edge_or_the_margin_is_left_out() -> anyhow::Result<()> {
        // A margin of 9: places stay 11 in from the edges — 39 at most.
        let mut wide = layout(Grid::Square);
        wide.margin = 9.0;
        let kept = places(&wide, &square(), Point2::ZERO)?;
        assert_eq!(kept.len(), 7 * 7 - 1);
        assert!(kept.iter().all(|p| p.x.abs() <= 39.0 && p.y.abs() <= 39.0));
        // A hole in the middle, 30 across: nothing in it, nor less than
        // the reach from its edge.
        let mut holed = square();
        holed.push(
            [(-15.0, -15.0), (15.0, -15.0), (15.0, 15.0), (-15.0, 15.0)]
                .map(|(x, y)| Point2::new(x, y))
                .to_vec(),
        );
        let kept = places(&layout(Grid::Square), &holed, Point2::new(-40.0, -40.0))?;
        assert!(kept.iter().all(|p| p.x.abs() > 17.0 || p.y.abs() > 17.0));
        // (The 5 × 5 of the middle, −20 … 20, are gone but the ring at
        // ±20: 9 × 9 − 1 less the 3 × 3 within ±10.)
        assert_eq!(kept.len(), 9 * 9 - 1 - 9);
        // Turned by 45°, a grid still fills the square but not past it.
        let mut turned = layout(Grid::Square);
        turned.angle = std::f64::consts::FRAC_PI_4;
        let kept = places(&turned, &square(), Point2::ZERO)?;
        assert!(kept.iter().all(|p| p.x.abs() <= 48.0 && p.y.abs() <= 48.0));
        assert!(kept.len() > 60);
        Ok(())
    }
}
