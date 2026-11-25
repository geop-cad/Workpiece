//! A geometric constraint solver, for sketches (and, later, assemblies).
//!
//! A [`System`] is variables and constraints; each constraint is a few
//! equations over a few of the variables, written once over a [`Scalar`]
//! so the solver gets values and exact derivatives from the same code.
//!
//! What the solver does around the Newton iteration:
//!
//! * **Least motion.** [`solve`] finds the solution nearest to where the
//!   variables are (the minimum-norm step, cadrs), so adding a constraint
//!   moves as little as possible.
//! * **Drags are pulls.** A drag names where some variables should go and
//!   how much that matters ([`Pull`]); the constraints always have the
//!   final say (geop). A pull that cannot be solved leaves the variables
//!   alone.
//! * **Branches are kept.** Solves start from the current values, and
//!   [`solve`] takes far pulls in steps, so a solution stays on its branch
//!   instead of flipping to a mirror image.
//! * **Components.** Variables that no constraint connects are solved
//!   apart, and components at rest are skipped: a drag solves only what it
//!   touches.
//! * **Diagnosis.** [`diagnose`] counts the degrees of freedom, says which
//!   variables can still move, and names the constraints that are
//!   redundant or in conflict (geomsolver).

mod linear;
pub mod scalar;

pub use scalar::{Dual, MAX_LOCALS, Scalar};

use linear::Factor;

/// Variables and the constraints between them.
pub trait System {
    /// How many variables there are.
    fn variables(&self) -> usize;

    /// How many constraints there are. Earlier constraints win: of
    /// constraints that say the same, the later ones are the redundant
    /// ones.
    fn constraints(&self) -> usize;

    /// The variables constraint `k` depends on (at most [`MAX_LOCALS`]).
    fn uses(&self, k: usize) -> Vec<usize>;

    /// The equations of constraint `k`: pushes onto `out` one residual
    /// per equation (zero when satisfied), given the values `x` of the
    /// variables [`uses`](Self::uses) lists, in that order.
    fn residuals<S: Scalar>(&self, k: usize, x: &[S], out: &mut Vec<S>);
}

/// Where variables should go, and how much each matters.
#[derive(Debug, Clone, PartialEq)]
pub struct Pull {
    /// Where each variable should be.
    pub target: Vec<f64>,
    /// How much it matters that it gets there, relative to the others
    /// staying where they are (weight 1).
    pub weight: Vec<f64>,
}

impl Pull {
    /// The weight of a variable the user holds.
    pub const HELD: f64 = 1e8;

    /// No pull: every variable should stay where it is in `x`.
    pub fn none(x: &[f64]) -> Self {
        Self {
            target: x.to_vec(),
            weight: vec![1.0; x.len()],
        }
    }

    /// The same pull, with variable `i` held at `target`.
    #[must_use]
    pub fn hold(mut self, i: usize, target: f64) -> Self {
        self.target[i] = target;
        self.weight[i] = Self::HELD;
        self
    }
}

/// How a solve went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    /// Whether every equation holds within the tolerance.
    pub converged: bool,
    /// The largest residual left.
    pub residual: f64,
    /// Newton steps taken, over all components.
    pub iterations: usize,
}

/// What can still move, and which constraints are too many.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnosis {
    /// The degrees of freedom left.
    pub dof: usize,
    /// For each variable, whether it can still move.
    pub free: Vec<bool>,
    /// Constraints that add nothing: earlier ones already say the same.
    pub redundant: Vec<usize>,
    /// Constraints that contradict earlier ones.
    pub conflicting: Vec<usize>,
}

/// One row of the Jacobian: an equation's derivatives.
struct Row {
    constraint: usize,
    residual: f64,
    entries: Vec<(usize, f64)>,
}

/// Every equation of the constraints `which`, with derivatives, at `x`.
fn rows<Sys: System>(system: &Sys, which: &[usize], x: &[f64]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut out = Vec::new();
    for &k in which {
        let vars = system.uses(k);
        assert!(
            vars.len() <= MAX_LOCALS,
            "constraint {k} uses too many variables"
        );
        let local: Vec<Dual> = vars
            .iter()
            .enumerate()
            .map(|(i, &v)| Dual::variable(x[v], i))
            .collect();
        out.clear();
        system.residuals(k, &local, &mut out);
        for r in &out {
            let entries = vars
                .iter()
                .enumerate()
                .map(|(i, &v)| (v, r.d[i]))
                .filter(|(_, d)| *d != 0.0)
                .collect();
            rows.push(Row {
                constraint: k,
                residual: r.v,
                entries,
            });
        }
    }
    rows
}

/// The largest residual of the constraints `which` at `x`.
fn residual<Sys: System>(system: &Sys, which: &[usize], x: &[f64]) -> f64 {
    let mut worst: f64 = 0.0;
    let mut out = Vec::new();
    for &k in which {
        let local: Vec<f64> = system.uses(k).iter().map(|&v| x[v]).collect();
        out.clear();
        system.residuals(k, &local, &mut out);
        worst = out.iter().fold(worst, |w, r| w.max(r.abs()));
    }
    if worst.is_nan() { f64::INFINITY } else { worst }
}

/// The constraints of each connected component (constraints sharing a
/// variable are connected), in order.
fn components<Sys: System>(system: &Sys) -> Vec<Vec<usize>> {
    let n = system.variables();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let uses: Vec<Vec<usize>> = (0..system.constraints()).map(|k| system.uses(k)).collect();
    for vars in &uses {
        for pair in vars.windows(2) {
            let (a, b) = (find(&mut parent, pair[0]), find(&mut parent, pair[1]));
            parent[a] = b;
        }
    }
    let mut of_root: Vec<Option<usize>> = vec![None; n];
    let mut out: Vec<Vec<usize>> = Vec::new();
    // Constraints without variables (between fixed things) each stand
    // alone.
    for (k, vars) in uses.iter().enumerate() {
        let Some(&first) = vars.first() else {
            out.push(vec![k]);
            continue;
        };
        let root = find(&mut parent, first);
        let slot = *of_root[root].get_or_insert_with(|| {
            out.push(Vec::new());
            out.len() - 1
        });
        out[slot].push(k);
    }
    out
}

/// The matrix `J W⁻¹ Jᵀ` of `rows` (dense, row-major).
fn normal_matrix(rows: &[Row], inverse_weight: &dyn Fn(usize) -> f64) -> Vec<f64> {
    let m = rows.len();
    let mut a = vec![0.0; m * m];
    for i in 0..m {
        for j in 0..=i {
            let mut sum = 0.0;
            for &(v, d) in &rows[i].entries {
                if let Some(&(_, e)) = rows[j].entries.iter().find(|(w, _)| *w == v) {
                    sum += d * e * inverse_weight(v);
                }
            }
            a[i * m + j] = sum;
            a[j * m + i] = sum;
        }
    }
    a
}

/// Newton steps on one component from `x` onto the constraints, each the
/// smallest step by the pull's weights (`dx = −W⁻¹Jᵀ(J W⁻¹ Jᵀ)⁻¹ r`): what
/// is held moves least, the rest as little as it can. Returns whether it
/// converged, and the steps taken.
fn settle<Sys: System>(
    system: &Sys,
    which: &[usize],
    x: &mut [f64],
    pull: &Pull,
    tolerance: f64,
) -> (bool, usize) {
    const MAX_STEPS: usize = 40;
    for step in 0..MAX_STEPS {
        let rows = rows(system, which, x);
        let worst = rows.iter().map(|r| r.residual.abs()).fold(0.0, f64::max);
        if !worst.is_finite() {
            return (false, step);
        }
        if worst <= tolerance {
            return (true, step);
        }
        let m = rows.len();
        let a = normal_matrix(&rows, &|v| 1.0 / pull.weight[v]);
        let b: Vec<f64> = rows.iter().map(|row| row.residual).collect();
        let lambda = Factor::new(a, m).solve(&b);
        for (row, l) in rows.iter().zip(&lambda) {
            for &(v, d) in &row.entries {
                x[v] -= d * l / pull.weight[v];
            }
        }
    }
    (residual(system, which, x) <= tolerance, MAX_STEPS)
}

/// Solves `system` from `x`, moving towards `pull`: what is pulled goes
/// to its target, and from there everything takes the smallest steps (by
/// the pull's weights) back onto the constraints. If that fails, the pull
/// is taken in smaller steps, so the solution stays on its branch. Components already solved and not
/// pulled are left alone.
///
/// On failure (no solution near, or a conflict) the components that
/// failed keep the values they had, and the report says so.
pub fn solve<Sys: System>(system: &Sys, x: &mut [f64], pull: &Pull, tolerance: f64) -> Report {
    assert_eq!(x.len(), system.variables());
    let mut report = Report {
        converged: true,
        residual: 0.0,
        iterations: 0,
    };
    for which in components(system) {
        let mut vars: Vec<usize> = which.iter().flat_map(|&k| system.uses(k)).collect();
        vars.sort_unstable();
        vars.dedup();
        let pulled = vars.iter().any(|&v| pull.target[v] != x[v]);
        if !pulled && residual(system, &which, x) <= tolerance {
            continue;
        }
        let start: Vec<f64> = vars.iter().map(|&v| x[v]).collect();
        // Whole, then in ever smaller steps towards the targets.
        let mut done = false;
        for pieces in [1usize, 4, 16] {
            let mut ok = true;
            for piece in 1..=pieces {
                let t = piece as f64 / pieces as f64;
                // What is pulled goes (part of the way) to its target; the
                // constraints then pull everything back onto them.
                for (&v, &x0) in vars.iter().zip(&start) {
                    if pull.target[v] != x0 {
                        x[v] = x0 + (pull.target[v] - x0) * t;
                    }
                }
                let (converged, steps) = settle(system, &which, x, pull, tolerance);
                report.iterations += steps;
                if !converged {
                    ok = false;
                    break;
                }
            }
            if ok {
                done = true;
                break;
            }
            for (&v, &x0) in vars.iter().zip(&start) {
                x[v] = x0;
            }
        }
        report.converged &= done;
        report.residual = report.residual.max(residual(system, &which, x));
    }
    // Variables no constraint uses go where they are pulled.
    let mut used = vec![false; x.len()];
    for k in 0..system.constraints() {
        for v in system.uses(k) {
            used[v] = true;
        }
    }
    for (v, used) in used.into_iter().enumerate() {
        if !used && pull.target[v].is_finite() {
            x[v] = pull.target[v];
        }
    }
    report
}

/// What can still move at `x`, and which constraints are too many. A
/// constraint is too many when its equations depend on earlier ones:
/// redundant if they hold anyway (within `tolerance`), conflicting if not.
pub fn diagnose<Sys: System>(system: &Sys, x: &[f64], tolerance: f64) -> Diagnosis {
    let n = system.variables();
    let all: Vec<usize> = (0..system.constraints()).collect();
    let rows = rows(system, &all, x);
    let m = rows.len();
    let factor = Factor::new(normal_matrix(&rows, &|_| 1.0), m);
    let mut diagnosis = Diagnosis {
        dof: n - factor.rank().min(n),
        free: vec![true; n],
        ..Diagnosis::default()
    };
    // A constraint says too much when rows of its equations depend on
    // earlier ones. It is in conflict if such a row does not hold; it is
    // redundant if none of its rows adds anything — one of whose rows
    // some still hold a freedom is needed, and is neither.
    let mut dropped_of: std::collections::BTreeMap<usize, usize> = Default::default();
    for &i in &factor.dropped {
        let k = rows[i].constraint;
        *dropped_of.entry(k).or_default() += 1;
        if rows[i].residual.abs() > tolerance && !diagnosis.conflicting.contains(&k) {
            diagnosis.conflicting.push(k);
        }
    }
    for (&k, &dropped) in &dropped_of {
        let all = rows.iter().filter(|row| row.constraint == k).count();
        if dropped == all && !diagnosis.conflicting.contains(&k) {
            diagnosis.redundant.push(k);
        }
    }
    // A variable is fixed when its axis lies in the row space of the
    // independent rows: 1 − ‖L⁻¹ J eᵢ‖² is how much of it is left free.
    let mut column = vec![0.0; m];
    for (v, free) in diagnosis.free.iter_mut().enumerate().take(n) {
        column.iter_mut().for_each(|c| *c = 0.0);
        let mut any = false;
        for (i, row) in rows.iter().enumerate() {
            if let Some(&(_, d)) = row.entries.iter().find(|(w, _)| *w == v) {
                column[i] = d;
                any = true;
            }
        }
        if any {
            let y = factor.forward(&column);
            let held: f64 = y.iter().map(|y| y * y).sum();
            *free = 1.0 - held > 1e-6;
        }
    }
    diagnosis
}

#[cfg(test)]
mod tests;
