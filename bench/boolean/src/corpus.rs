//! Rounds on public parts, to be held against another kernel's: of each
//! STEP file given, the longest sharp edges of its first solid are
//! rounded one at a time (by a hundredth of the part's size, or less
//! where the faces beside the edge are narrow), and what each round took
//! away or added is
//! written down — where the edge is, the radius, and the volume and
//! area before and after — for a script to do the same edges with the
//! other kernel and compare what came out (`README.md`, "Rounds on
//! public parts").

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use shapecraft_boolean::model::Walk;
use shapecraft_boolean::{Solid, measure};
use shapecraft_core::math::Point3;
use shapecraft_shape::{BlendShape, By, Section, blend_solid};

/// How many edges of a part are rounded.
const EDGES: usize = 4;

/// The outward normals of the two faces along edge `e`, at its middle.
fn normals(solid: &Solid, e: usize) -> Vec<Point3> {
    let t = solid.edges[e].middle();
    let mut out = Vec::new();
    for face in &solid.faces {
        for step in face.loops.iter().flatten() {
            if matches!(step.walk, Walk::Edge { edge, .. } if edge == e) {
                let uv = step.pcurve.point_at(t);
                let (_, du, dv) = face.surface.derivatives_at(uv.x, uv.y);
                let n = du.cross(dv).normalize_or_zero();
                out.push(if face.outward { n } else { -n });
            }
        }
    }
    out
}

fn length(solid: &Solid, e: usize) -> f64 {
    let edge = &solid.edges[e];
    let (a, b) = edge.range;
    let at = |k: usize| edge.point(a + (b - a) * k as f64 / 32.0);
    (0..32).map(|k| at(k).distance(at(k + 1))).sum()
}

/// How far the middle of edge `e` is from the other edges of its two
/// faces that do not start or end where it does: about how wide the
/// faces are there.
fn room(solid: &Solid, e: usize) -> f64 {
    let edge = &solid.edges[e];
    let at = edge.point(edge.middle());
    let mut least = f64::INFINITY;
    for face in &solid.faces {
        let steps = || face.loops.iter().flatten();
        if !steps().any(|s| matches!(s.walk, Walk::Edge { edge, .. } if edge == e)) {
            continue;
        }
        for step in steps() {
            let Walk::Edge { edge: other, .. } = step.walk else {
                continue;
            };
            let o = &solid.edges[other];
            if other == e || o.ends.iter().any(|v| edge.ends.contains(v)) {
                continue;
            }
            let (a, b) = o.range;
            for k in 0..=16 {
                least = least.min(o.point(a + (b - a) * f64::from(k) / 16.0).distance(at));
            }
        }
    }
    least
}

/// Rounds the longest sharp edges of each of `files`' first solid, and
/// writes what came of it to `out`, a line an edge.
pub fn run(files: &[PathBuf], out: &Path) -> Result<(), String> {
    let mut table = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut line = |text: String| writeln!(table, "{text}").map_err(|e| e.to_string());
    line("file\tedge\tx\ty\tz\tradius\tdiagonal\tvolume\tarea\tstatus\tvolume_after\tarea_after\tms\twhy".into())?;
    for file in files {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let solid = match std::fs::read(file)
            .map_err(|e| e.to_string())
            .and_then(|bytes| shapecraft_ops_step::import(&bytes).map_err(|e| format!("{e:#}")))
        {
            Ok(found) => match found.bodies.into_iter().next() {
                Some(body) => body.solid,
                None => {
                    eprintln!("{name}: no solid");
                    continue;
                }
            },
            Err(why) => {
                eprintln!("{name}: {why}");
                continue;
            }
        };
        let (lo, hi) = solid.extent();
        let diagonal = (hi - lo).length();
        let most = diagonal * 0.01;
        let before = measure(&solid);
        // The sharp edges, the longest first: those between two faces
        // that meet at an angle, and long enough to round.
        let mut sharp: Vec<(f64, usize)> = (0..solid.edges.len())
            .filter(|&e| {
                let n = normals(&solid, e);
                n.len() == 2 && n[0].dot(n[1]) < 0.94
            })
            .map(|e| (length(&solid, e), e))
            .filter(|(long, _)| *long > 6.0 * most)
            .collect();
        sharp.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (k, &(_, e)) in sharp.iter().take(EDGES).enumerate() {
            let edge = &solid.edges[e];
            let at = edge.point(edge.middle());
            // A hundredth of the part's size, or as much less as there
            // is room for beside the edge.
            let radius = most.min(room(&solid, e) * 0.3);
            let shape = BlendShape::Fillet {
                by: By::Radius,
                size: radius,
                until: None,
                section: Section::Circular,
                rho: 0.5,
            };
            let started = Instant::now();
            let made = std::panic::catch_unwind(|| {
                blend_solid(&solid, "round", &[e], &shape, true).map_err(|e| format!("{e:#}"))
            })
            .unwrap_or_else(|_| Err("crashed".to_owned()))
            .and_then(|made| crate::scene::sound(&made));
            let ms = started.elapsed().as_secs_f64() * 1e3;
            let (status, after, why) = match made {
                Ok((volume, area)) => ("ok", (volume, area), String::new()),
                Err(why) => ("refused", (0.0, 0.0), why.replace(['\t', '\n'], " ")),
            };
            line(format!(
                "{name}\t{k}\t{:.9}\t{:.9}\t{:.9}\t{radius:.9}\t{diagonal:.9}\t{:.9}\t{:.9}\t{status}\t{:.9}\t{:.9}\t{ms:.1}\t{why}",
                at.x, at.y, at.z, before.volume, before.area, after.0, after.1
            ))?;
            eprintln!("{name} edge {k}: {status} in {ms:.0} ms {why}");
        }
    }
    Ok(())
}
