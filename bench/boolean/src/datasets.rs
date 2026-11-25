//! The public STEP files the suite runs on, and the cases made of them.
//!
//! `datasets.tsv` lists each file with its address and SHA-256, so that
//! every run is on the very same bytes:
//!
//! * `nist`: the test parts of NIST's MBE PMI Validation and Conformance
//!   Testing project (CTC, FTC, STC), exported from several CAD systems;
//!   free of restrictions.
//! * `kicad`: packages of the KiCad 3D model library (CC-BY-SA 4.0 with
//!   an exception), written by Open CASCADE.
//! * `freecad`: parts of the FreeCAD library (CC-BY 3.0).
//! * `kicad-more`, `freecad-more`: several hundred more files of the
//!   same two libraries, across all their directories. These are only
//!   imported (`step-import`), not combined.
//!
//! They are downloaded by `boolean-bench fetch` into
//! `target/bench-data`, and are not part of the repository.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, ensure};
use glam::DQuat;
use shapecraft_boolean::{Op, Solid, measure};

use crate::scene::{Scene, bounds, consistent, volume_of};
use crate::{Case, Grade};

pub fn data() -> PathBuf {
    crate::root().join("../../target/bench-data")
}

/// The files of the list: set, name, SHA-256 and address of each.
fn listed() -> Vec<[String; 4]> {
    include_str!("../datasets.tsv")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut parts = l.split('\t').map(str::to_owned);
            Some([parts.next()?, parts.next()?, parts.next()?, parts.next()?])
        })
        .collect()
}

/// The SHA-256 of `bytes`, in hexadecimal.
fn sha256(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(bytes.len() as u64 * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// Downloads every listed file that is not there yet, and checks each
/// against its SHA-256.
pub fn fetch() -> anyhow::Result<()> {
    for [set, name, sum, url] in listed() {
        let dir = data().join(&set);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&name);
        if !path.exists() {
            println!("fetching {set}/{name}");
            let status = Command::new("curl")
                .args(["-sSL", "--fail", "-m", "300", "-A", "curl", "-o"])
                .arg(&path)
                .arg(&url)
                .status()
                .context("running curl")?;
            ensure!(status.success(), "curl could not fetch {url}");
        }
        let got = sha256(&std::fs::read(&path)?);
        ensure!(
            got == sum,
            "{} is not the file listed: its SHA-256 is {got}",
            path.display()
        );
        if name.ends_with(".zip") {
            let status = Command::new("unzip")
                .args(["-oq"])
                .arg(&path)
                .arg("-d")
                .arg(&dir)
                .status()
                .context("running unzip")?;
            ensure!(
                status.success(),
                "unzip could not unpack {}",
                path.display()
            );
        }
    }
    println!("the datasets are in {}", data().display());
    Ok(())
}

fn step_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            step_files(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "step" | "stp"))
        {
            out.push(path);
        }
    }
}

/// The solids of `path`, imported once and shared by its cases.
fn solids(path: &Path) -> Result<Vec<Solid>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let found = shapecraft_ops_step::import(&bytes).map_err(|e| format!("{e:#}"))?;
    if let Some(why) = found.refused.first() {
        return Err(why.clone());
    }
    if found.bodies.is_empty() {
        return Err("no solids".to_owned());
    }
    Ok(found.bodies.into_iter().map(|b| b.solid).collect())
}

/// The cases of the downloaded files: each file's import, and booleans
/// of its first solid — halved by a box, drilled through, and combined
/// with itself.
pub fn cases() -> Vec<Case> {
    let mut files = Vec::new();
    step_files(&data(), &mut files);
    let mut out = Vec::new();
    for path in files {
        // (The published benchmark's parts are cases of their own.)
        if path.starts_with(data().join(crate::conformance::SET)) {
            continue;
        }
        let name = path
            .strip_prefix(data())
            .unwrap_or(&path)
            .to_string_lossy()
            .replace(' ', "_");
        // Imported when its first case runs; kept for the others.
        let cell: Arc<std::sync::OnceLock<Result<Vec<Solid>, String>>> =
            Arc::new(std::sync::OnceLock::new());
        let load = {
            let (cell, path) = (cell.clone(), path.clone());
            move || cell.get_or_init(|| solids(&path)).clone()
        };
        let import = load.clone();
        out.push(Case {
            category: "step-import",
            name: name.clone(),
            run: Box::new(move || match import() {
                Err(why) => Grade::Refused(why),
                Ok(solids) => {
                    // Each a valid closed solid, whose faces enclose
                    // a volume, outwards.
                    for solid in &solids {
                        match crate::scene::sound(solid) {
                            Err(why) => return Grade::Wrong(why),
                            Ok((v, _)) if v <= 0.0 => {
                                return Grade::Wrong(format!("a solid of volume {v}"));
                            }
                            Ok(_) => {}
                        }
                    }
                    Grade::Pass
                }
            }),
        });
        // The larger corpus (the sets called `…-more`) is only opened:
        // hundreds of files, to see that they are read. The booleans
        // run on the smaller sets.
        if name
            .split('/')
            .next()
            .is_some_and(|set| set.ends_with("-more"))
        {
            continue;
        }
        let with = |what: &'static str, run: fn(&Solid) -> anyhow::Result<Grade>| {
            let load = load.clone();
            Case {
                category: "step",
                name: format!("{name}/{what}"),
                run: Box::new(move || match load() {
                    Err(why) => Grade::Refused(format!("import: {why}")),
                    Ok(solids) => {
                        run(&solids[0]).unwrap_or_else(|e| Grade::Crashed(format!("setup: {e:#}")))
                    }
                }),
            }
        };
        out.push(with("halved", |solid| {
            // A box over the half of the solid beyond its middle along
            // x, a little off it so that it cuts no face along itself.
            let (lo, hi) = bounds(solid);
            let (mid, size) = ((lo + hi) / 2.0, hi - lo);
            let at = mid + glam::DVec3::new(size.x * 0.553, size.y * 0.013, size.z * 0.007);
            let cutter = Scene::new().cube(at.into(), (size * 1.1).into(), DQuat::IDENTITY)?;
            Ok(consistent(solid, &cutter, None))
        }));
        out.push(with("sliced", |solid| {
            // A slab through its middle, turned off its axes.
            let (lo, hi) = bounds(solid);
            let (mid, size) = ((lo + hi) / 2.0, hi - lo);
            let turn = DQuat::from_axis_angle(glam::DVec3::new(0.3, 1.0, 0.2).normalize(), 0.37);
            let reach = size.length() * 1.5;
            let cutter =
                Scene::new().cube(mid.into(), [reach, reach, size.min_element() * 0.31], turn)?;
            Ok(consistent(solid, &cutter, None))
        }));
        out.push(with("drilled", |solid| {
            // A hole through its middle along z, a third of its width.
            let (lo, hi) = bounds(solid);
            let (mid, size) = ((lo + hi) / 2.0, hi - lo);
            let radius = size.x.min(size.y) * 0.17;
            let base = [
                mid.x + size.x * 0.011,
                mid.y - size.y * 0.007,
                lo.z - size.z * 0.2,
            ];
            let drill =
                Scene::new().cylinder(base, radius, size.z * 1.4, DQuat::from_rotation_z(0.3))?;
            Ok(consistent(solid, &drill, None))
        }));
        out.push(with("with-itself", |solid| {
            let volume = measure(solid).volume;
            let near = |v: f64, wanted: f64| (v - wanted).abs() <= crate::scene::CLOSE * volume;
            for (op, wanted) in [(Op::Union, volume), (Op::Intersect, volume), (Op::Cut, 0.0)] {
                match volume_of(op, solid, solid) {
                    Err(grade) => return Ok(grade),
                    Ok(v) if !near(v, wanted) => {
                        return Ok(Grade::Wrong(format!(
                            "{op:?} with itself is {v}, not {wanted}"
                        )));
                    }
                    Ok(_) => {}
                }
            }
            Ok(Grade::Pass)
        }));
    }
    out
}
