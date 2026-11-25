//! Builds a part file step by step without a window, and says how each
//! step went: `cargo run --release -p workpiece-part --example build --
//! playground/part9.workpiece` (and, with a second path, writes its solids
//! there as STEP).

use std::collections::BTreeMap;
use std::time::Instant;

use workpiece_part::{Part, Workspace};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("which file?"))?;
    let part = Part::from_text(&std::fs::read_to_string(&path)?)?;
    let steps = part.program.len();
    let ids: Vec<String> = part.program.iter().map(|op| op.id().to_owned()).collect();
    let unit = part.unit;
    let workspace = Workspace::new(BTreeMap::from([(path.clone(), part)]));
    let mut failed = 0;
    for n in 1..=steps {
        let started = Instant::now();
        match workspace.build_steps(&path, n) {
            Ok(world) => {
                println!("ok      {:<16} {:?}", ids[n - 1], started.elapsed());
                // (Vertices that only part an edge in two that runs
                // on smoothly, for one who asks: `SEAMS=1`.)
                if std::env::var("SEAMS").is_ok() {
                    needless(&world.topo);
                }
                // (What it made, for one who asks: `NAMES=extrude3/`.)
                if let Ok(prefix) = std::env::var("NAMES") {
                    for (name, id) in world.topo.names().with_prefix(&prefix) {
                        let at = workpiece_core::world::topo::VertexId::try_from(id)
                            .ok()
                            .and_then(|v| world.topo.vertex(v))
                            .map(|v| {
                                format!(" at {:.4} {:.4} {:.4}", v.point.x, v.point.y, v.point.z)
                            });
                        println!("          {name}{}", at.unwrap_or_default());
                    }
                }
            }
            Err(error) => {
                failed += 1;
                println!(
                    "FAILED  {:<16} {:?}: {error:#}",
                    ids[n - 1],
                    started.elapsed()
                );
            }
        }
    }
    println!("{} of {steps} steps failed", failed);
    // With a second path, the part's solids are written there as STEP,
    // and their volumes said.
    if let Some(out) = std::env::args().nth(2) {
        let world = workspace.build_steps(&path, steps)?;
        let topo = &world.topo;
        let mut bodies = Vec::new();
        for (id, _) in topo.bodies().iter() {
            let Ok(solid) = workpiece_boolean::read(topo, id) else {
                continue;
            };
            let name = topo.name_of(id).unwrap_or("body").to_owned();
            println!(
                "{name}: volume {}",
                workpiece_boolean::measure(&solid).volume
            );
            bodies.push((name, solid));
        }
        std::fs::write(&out, workpiece_ops_step::export("part", &bodies, unit)?)?;
        println!("written to {out}");
    }
    Ok(())
}

/// Says, of every solid of `topo`, the vertices where just two edges
/// meet that go on in one direction: what could be one edge.
fn needless(topo: &workpiece_core::world::topo::Topo) {
    for (id, _) in topo.bodies().iter() {
        let Ok(solid) = workpiece_boolean::read(topo, id) else {
            continue;
        };
        for (v, vertex) in solid.vertices.iter().enumerate() {
            let at: Vec<_> = solid
                .edges
                .iter()
                .filter(|e| e.ends[0] == v || e.ends[1] == v)
                .collect();
            if at.len() != 2 || at.iter().any(|e| e.ends[0] == e.ends[1]) {
                continue;
            }
            let way = |e: &workpiece_boolean::model::Edge| {
                let (a, b) = e.range;
                let t = if e.ends[0] == v { a } else { b };
                let d = e.curve.derivative_at(t).1.normalize();
                if e.ends[0] == v { d } else { -d }
            };
            let turn = way(at[0]).dot(way(at[1]));
            if turn < -0.9999 {
                let p = vertex.point;
                println!(
                    "          needless {} at {:.4} {:.4} {:.4} between {} and {}",
                    vertex.name.as_deref().unwrap_or("?"),
                    p.x,
                    p.y,
                    p.z,
                    at[0].name.as_deref().unwrap_or("?"),
                    at[1].name.as_deref().unwrap_or("?"),
                );
            }
        }
    }
}
