//! The benchmark suite of the booleans.
//!
//! Every case builds two solids, combines them, and is graded like a
//! case of brep-bench: **pass** if the result is a valid closed solid
//! with the volume (and area, where it is known) the case expects,
//! **refused** if the boolean declined with a reason, **wrong** if it
//! gave something else, **crashed** if it panicked. Refused ranks above
//! wrong: a wrong answer is the one thing a kernel must not give.
//!
//! The cases:
//!
//! * `box`, `cylinder`, `sphere`, `turned`: pairs of solids with volumes
//!   in closed form — crossing, nested, apart, through and blind.
//! * `coincident`: faces in one plane, on one cylinder, whole solids the
//!   same.
//! * `tangent`: solids that touch along a line or at a point.
//! * `nearmiss`: faces brought within fractions and multiples of the
//!   tolerance of each other (a perturbation sweep).
//! * `chain`: many booleans on one body.
//! * `random`: random pairs of boxes, cylinders and balls, checked by
//!   what must hold of any boolean (`|A ∪ B| = |A| + |B| − |A ∩ B|`, …)
//!   and against a count of random points.
//! * `step`: solids of public STEP files (see `datasets.tsv`), halved,
//!   drilled and combined with themselves.
//!
//! ```text
//! cargo run --release -p shapecraft-boolean-bench             # run everything
//! cargo run --release -p shapecraft-boolean-bench -- fetch    # download the datasets
//! cargo run --release -p shapecraft-boolean-bench -- box tangent   # some categories
//! cargo run --release -p shapecraft-boolean-bench -- --bless  # accept the results as the baseline
//! ```
//!
//! It exits with an error if a case that passed in `baseline.tsv` no
//! longer does, or if more cases are wrong than there.

mod brepbench;
mod brepbench_expected;
mod cases;
mod conformance;
mod corpus;
mod datasets;
mod features;
mod mesh;
mod scene;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How a case came out.
#[derive(Debug, Clone, PartialEq)]
pub enum Grade {
    /// A valid solid with the expected measures.
    Pass,
    /// Declined, with the reason.
    Refused(String),
    /// A result that is not what was expected.
    Wrong(String),
    /// A panic.
    Crashed(String),
}

impl Grade {
    fn word(&self) -> &'static str {
        match self {
            Grade::Pass => "pass",
            Grade::Refused(_) => "refused",
            Grade::Wrong(_) => "wrong",
            Grade::Crashed(_) => "crashed",
        }
    }

    fn why(&self) -> &str {
        match self {
            Grade::Pass => "",
            Grade::Refused(s) | Grade::Wrong(s) | Grade::Crashed(s) => s,
        }
    }
}

/// One case: its category, its name, and what runs and grades it.
pub struct Case {
    /// The category.
    pub category: &'static str,
    /// The name, unique in the category.
    pub name: String,
    /// Runs it.
    pub run: Box<dyn Fn() -> Grade + Send + Sync>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "fetch") {
        if let Err(e) = datasets::fetch() {
            eprintln!("fetch failed: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    // As a reader for step-conformance: `step-read OUT TOLERANCE FILE`
    // reads a STEP file and writes its triangles as that benchmark's
    // scorer takes them.
    if args.first().is_some_and(|a| a == "step-read") {
        let (Some(out), Some(tolerance), Some(file)) = (
            args.get(1),
            args.get(2).and_then(|t| t.parse::<f64>().ok()),
            args.get(3),
        ) else {
            eprintln!("usage: boolean-bench step-read OUT TOLERANCE FILE");
            std::process::exit(2);
        };
        if let Err(e) = conformance::adapter(
            std::path::Path::new(out),
            tolerance,
            std::path::Path::new(file),
        ) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    // `fillet-corpus OUT.tsv FILE…` rounds edges of public parts, for
    // another kernel to do the same and the two to be compared.
    if args.first().is_some_and(|a| a == "fillet-corpus") {
        let Some(out) = args.get(1) else {
            eprintln!("usage: boolean-bench fillet-corpus OUT.tsv FILE…");
            std::process::exit(2);
        };
        let files: Vec<std::path::PathBuf> = args[2..].iter().map(Into::into).collect();
        if let Err(e) = corpus::run(&files, std::path::Path::new(out)) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    // As a runner of brep-bench: `brep-run CASE OUT.stl` builds one of
    // its cases, writes its triangles and says what it measures.
    if args.first().is_some_and(|a| a == "brep-run") {
        if args.iter().any(|a| a == "--version") {
            println!("shapecraft {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        let (Some(case), Some(out)) = (args.get(1), args.get(2)) else {
            eprintln!("usage: boolean-bench brep-run CASE OUT.stl");
            std::process::exit(2);
        };
        let refuse = |why: &str| {
            println!("{{\"status\": \"refused\", \"reason\": {why:?}}}");
        };
        match brepbench::build(case) {
            Err(Grade::Refused(why)) => refuse(&why),
            Err(other) => {
                eprintln!("{}", other.why());
                std::process::exit(1);
            }
            Ok(solid) => match scene::sound(&solid) {
                Err(why) => refuse(&format!("the result is not a valid solid: {why}")),
                Ok((volume, area)) => {
                    let (lo, hi) = solid.extent();
                    // Fine enough for the solid's thinnest way too — to
                    // start with: what follows makes it finer if the
                    // triangles do not enclose what the solid does.
                    let size = hi - lo;
                    let mut sag = (size.length() * 1e-3).min(size.min_element() * 0.02);
                    let mut triangles = mesh::triangles(&solid, sag);
                    // Finer, as long as the triangles enclose noticeably
                    // less or more than the solid does: what is thin
                    // without the box around it being so — a pipe
                    // along a helix — needs it.
                    for _ in 0..5 {
                        let enclosed: f64 = triangles
                            .iter()
                            .map(|[a, b, c]| a.dot(b.cross(*c)) / 6.0)
                            .sum();
                        if (enclosed - volume).abs() <= volume.abs() * 8e-3 {
                            break;
                        }
                        // (Not without end: a blade thinner than
                        // anything is told apart by is left as it is.)
                        let least = (size.length() * 1e-4).min(size.min_element() * 0.004);
                        // (What the triangles are off by goes about as
                        // the sag does: finer by as much as it takes.)
                        let off = (enclosed - volume).abs() / volume.abs();
                        let finer = (sag * (4e-3 / off).clamp(0.3, 0.5)).max(least);
                        if finer >= sag {
                            break;
                        }
                        sag = finer;
                        triangles = mesh::triangles(&solid, sag);
                    }
                    if let Err(e) = mesh::write_stl(std::path::Path::new(out), &triangles) {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                    println!(
                        "{{\"status\": \"ok\", \"volume\": {volume:e}, \"area\": {area:e}, \"faces\": {{\"other\": {}}}, \"valid\": true}}",
                        solid.faces.len()
                    );
                }
            },
        }
        return;
    }
    let bless = args.iter().any(|a| a == "--bless");
    let verbose = args.iter().any(|a| a == "--verbose");
    let only: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let mut all = cases::all();
    all.extend(brepbench::cases());
    all.extend(features::cases());
    all.extend(datasets::cases());
    all.extend(conformance::cases());
    if !only.is_empty() {
        all.retain(|c| {
            only.iter()
                .any(|o| c.category == o.as_str() || c.name.contains(o.as_str()))
        });
    }
    // A panic is a grade, not the end of the run.
    std::panic::set_hook(Box::new(|_| {}));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Grade, Duration)>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let clock = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = all.get(k) else {
                        break;
                    };
                    if verbose {
                        eprintln!("running {}/{}", case.category, case.name);
                    }
                    let started = Instant::now();
                    let grade =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (case.run)()))
                            .unwrap_or_else(|panic| {
                                let what = panic
                                    .downcast_ref::<String>()
                                    .cloned()
                                    .or_else(|| {
                                        panic.downcast_ref::<&str>().map(|s| (*s).to_owned())
                                    })
                                    .unwrap_or_else(|| "a panic".to_owned());
                                Grade::Crashed(what)
                            });
                    if verbose {
                        let took = started.elapsed();
                        eprintln!("done    {}/{} in {took:.2?}", case.category, case.name);
                    }
                    if let Ok(mut r) = results.lock() {
                        r.push((k, grade, started.elapsed()));
                    }
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_default();
    results.sort_by_key(|r| r.0);

    // The table: per category, how many of each grade, and the middle
    // time.
    let mut table: BTreeMap<&str, ([usize; 4], Vec<Duration>)> = BTreeMap::new();
    for (k, grade, time) in &results {
        let row = table.entry(all[*k].category).or_default();
        let column = match grade {
            Grade::Pass => 0,
            Grade::Refused(_) => 1,
            Grade::Wrong(_) => 2,
            Grade::Crashed(_) => 3,
        };
        row.0[column] += 1;
        row.1.push(*time);
    }
    println!(
        "{:<12} {:>6} {:>6} {:>8} {:>6} {:>8} {:>10}",
        "category", "cases", "pass", "refused", "wrong", "crashed", "median"
    );
    let mut total = [0; 4];
    for (category, (counts, times)) in &mut table {
        times.sort();
        println!(
            "{:<12} {:>6} {:>6} {:>8} {:>6} {:>8} {:>10.2?}",
            category,
            counts.iter().sum::<usize>(),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            times[times.len() / 2]
        );
        for k in 0..4 {
            total[k] += counts[k];
        }
    }
    println!(
        "{:<12} {:>6} {:>6} {:>8} {:>6} {:>8} {:>10.2?}",
        "all",
        total.iter().sum::<usize>(),
        total[0],
        total[1],
        total[2],
        total[3],
        clock.elapsed()
    );
    println!();
    for (k, grade, time) in &results {
        if *grade != Grade::Pass {
            let case = &all[*k];
            println!(
                "{:<8} {}/{} ({time:.1?}): {}",
                grade.word(),
                case.category,
                case.name,
                grade.why()
            );
        }
    }

    // What came out, and how it compares with what was accepted.
    let mut lines = String::from("# case\tgrade\n");
    for (k, grade, _) in &results {
        lines.push_str(&format!(
            "{}/{}\t{}\n",
            all[*k].category,
            all[*k].name,
            grade.word()
        ));
    }
    let out = root().join("../../target/bench-results");
    let _ = std::fs::create_dir_all(&out);
    let _ = std::fs::write(out.join("boolean.tsv"), &lines);
    let baseline_path = root().join("baseline.tsv");
    if bless {
        // Only what ran is replaced; the rest of the baseline stays.
        let mut merged: BTreeMap<String, String> = read_baseline(&baseline_path);
        for (k, grade, _) in &results {
            merged.insert(
                format!("{}/{}", all[*k].category, all[*k].name),
                grade.word().to_owned(),
            );
        }
        let mut text = String::from("# case\tgrade\n");
        for (case, grade) in &merged {
            text.push_str(&format!("{case}\t{grade}\n"));
        }
        match std::fs::write(&baseline_path, text) {
            Ok(()) => println!("\nbaseline written: {}", baseline_path.display()),
            Err(e) => eprintln!("\nbaseline not written: {e}"),
        }
        return;
    }
    let baseline = read_baseline(&baseline_path);
    let mut worse = Vec::new();
    for (k, grade, _) in &results {
        let name = format!("{}/{}", all[*k].category, all[*k].name);
        let before = baseline.get(&name).map(String::as_str);
        let regressed = match (before, grade) {
            (Some("pass"), g) => *g != Grade::Pass,
            (Some("refused"), Grade::Wrong(_) | Grade::Crashed(_)) => true,
            (None, Grade::Wrong(_) | Grade::Crashed(_)) => true,
            _ => false,
        };
        if regressed {
            worse.push(format!(
                "{name}: {} (was {})",
                grade.word(),
                before.unwrap_or("new")
            ));
        }
    }
    if !worse.is_empty() {
        println!("\n{} case(s) worse than the baseline:", worse.len());
        for line in &worse {
            println!("  {line}");
        }
        std::process::exit(1);
    }
    println!("\nno case is worse than the baseline");
}

fn read_baseline(path: &std::path::Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let (case, grade) = l.split_once('\t')?;
            Some((case.to_owned(), grade.to_owned()))
        })
        .collect()
}
