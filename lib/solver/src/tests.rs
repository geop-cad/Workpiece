use super::*;

/// Points in the plane (variables 2i, 2i+1) with a few kinds of
/// constraint.
#[derive(Default)]
struct Points {
    n: usize,
    constraints: Vec<C>,
}

enum C {
    /// |a − b| = length.
    Distance(usize, usize, f64),
    /// a is at this position.
    At(usize, f64, f64),
    /// a and b have the same y.
    Level(usize, usize),
}

impl System for Points {
    fn variables(&self) -> usize {
        2 * self.n
    }

    fn constraints(&self) -> usize {
        self.constraints.len()
    }

    fn uses(&self, k: usize) -> Vec<usize> {
        match self.constraints[k] {
            C::Distance(a, b, _) => vec![2 * a, 2 * a + 1, 2 * b, 2 * b + 1],
            C::At(a, ..) => vec![2 * a, 2 * a + 1],
            C::Level(a, b) => vec![2 * a + 1, 2 * b + 1],
        }
    }

    fn residuals<S: Scalar>(&self, k: usize, x: &[S], out: &mut Vec<S>) {
        match self.constraints[k] {
            C::Distance(_, _, length) => {
                let (dx, dy) = (x[2] - x[0], x[3] - x[1]);
                out.push((dx * dx + dy * dy).sqrt() - S::constant(length));
            }
            C::At(_, px, py) => {
                out.push(x[0] - S::constant(px));
                out.push(x[1] - S::constant(py));
            }
            C::Level(..) => out.push(x[1] - x[0]),
        }
    }
}

const TOLERANCE: f64 = 1e-10;

#[test]
fn solving_moves_as_little_as_it_can() {
    // Two free points 3 apart must be 5 apart: both move 1 outwards.
    let system = Points {
        n: 2,
        constraints: vec![C::Distance(0, 1, 5.0)],
    };
    let mut x = vec![0.0, 0.0, 3.0, 0.0];
    let report = solve(
        &system,
        &mut x,
        &Pull::none(&[0.0, 0.0, 3.0, 0.0]),
        TOLERANCE,
    );
    assert!(report.converged, "{report:?}");
    assert!(
        (x[0] + 1.0).abs() < 1e-9 && (x[2] - 4.0).abs() < 1e-9,
        "{x:?}"
    );
    assert!(x[1].abs() < 1e-9 && x[3].abs() < 1e-9);
    // Solved, it stays put: no steps.
    let again = solve(&system, &mut x.clone(), &Pull::none(&x), TOLERANCE);
    assert_eq!(again.iterations, 0);
}

#[test]
fn drags_pull_and_the_constraints_have_the_final_say() {
    // A pendulum: point 0 pinned at the origin, point 1 at distance 2.
    let system = Points {
        n: 2,
        constraints: vec![C::At(0, 0.0, 0.0), C::Distance(0, 1, 2.0)],
    };
    let mut x = vec![0.0, 0.0, 2.0, 0.0];
    // Pulled far up and a little right, it goes to the circle's nearest
    // point, on its way there.
    let pull = Pull::none(&x).hold(2, 3.0).hold(3, 30.0);
    let report = solve(&system, &mut x, &pull, TOLERANCE);
    assert!(report.converged, "{report:?}");
    let radius = x[2].hypot(x[3]);
    assert!((radius - 2.0).abs() < 1e-9);
    let nearest = (
        3.0f64.atan2(30.0).sin() * 2.0,
        3.0f64.atan2(30.0).cos() * 2.0,
    );
    assert!(
        (x[2] - nearest.0).abs() < 1e-3 && (x[3] - nearest.1).abs() < 1e-3,
        "{x:?}"
    );
    assert!(x[0].abs() < 1e-9 && x[1].abs() < 1e-9);
}

#[test]
fn far_pulls_stay_on_their_branch() {
    // A triangle on a pinned base: the apex is above. Shortening the
    // sides must keep it above, not flip it below.
    let mut system = Points {
        n: 3,
        constraints: vec![
            C::At(0, 0.0, 0.0),
            C::At(1, 4.0, 0.0),
            C::Distance(0, 2, 5.0),
            C::Distance(1, 2, 5.0),
        ],
    };
    let mut x = vec![0.0, 0.0, 4.0, 0.0, 2.0, 21f64.sqrt()];
    system.constraints[2] = C::Distance(0, 2, 2.5);
    system.constraints[3] = C::Distance(1, 2, 2.5);
    let pull = Pull::none(&x);
    let report = solve(&system, &mut x, &pull, TOLERANCE);
    assert!(report.converged);
    assert!(
        (x[4] - 2.0).abs() < 1e-9 && (x[5] - 1.5).abs() < 1e-9,
        "{x:?}"
    );
    // Sides too short to meet: no solution, and nothing moves.
    system.constraints[2] = C::Distance(0, 2, 1.0);
    system.constraints[3] = C::Distance(1, 2, 1.0);
    let before = x.clone();
    let report = solve(&system, &mut x, &Pull::none(&before), TOLERANCE);
    assert!(!report.converged);
    assert_eq!(x, before);
}

#[test]
fn components_at_rest_are_left_alone() {
    // Two separate pairs; only the first is off.
    let system = Points {
        n: 4,
        constraints: vec![C::Distance(0, 1, 2.0), C::Distance(2, 3, 1.0)],
    };
    let mut x = vec![0.0, 0.0, 1.0, 0.0, 10.0, 0.0, 11.0, 0.0];
    let pull = Pull::none(&x);
    let report = solve(&system, &mut x, &pull, TOLERANCE);
    assert!(report.converged);
    assert_eq!(&x[4..], &[10.0, 0.0, 11.0, 0.0]);
    assert!((x[2] - x[0] - 2.0).abs() < 1e-9);
}

#[test]
fn diagnosis_counts_freedom_and_names_what_is_too_much() {
    let mut system = Points {
        n: 2,
        constraints: vec![C::At(0, 0.0, 0.0), C::Distance(0, 1, 2.0)],
    };
    let x = vec![0.0, 0.0, 2.0, 0.0];
    let d = diagnose(&system, &x, TOLERANCE);
    // The second point swings: one freedom, in y where it stands.
    assert_eq!(d.dof, 1);
    assert_eq!(d.free, [false, false, false, true]);
    assert!(d.redundant.is_empty() && d.conflicting.is_empty());

    system.constraints.push(C::Level(0, 1));
    let d = diagnose(&system, &x, TOLERANCE);
    assert_eq!(d.dof, 0);
    assert_eq!(d.free, [false; 4]);

    // Saying it again is redundant; saying otherwise conflicts.
    system.constraints.push(C::Distance(0, 1, 2.0));
    system.constraints.push(C::At(1, 2.0, 1.0));
    let d = diagnose(&system, &x, TOLERANCE);
    assert_eq!(d.redundant, [3]);
    assert_eq!(d.conflicting, [4]);
}

#[test]
fn a_constraint_that_still_holds_a_freedom_is_not_redundant() {
    // A point held level with the origin; then pinned to (2, 0): the
    // pin says the level again and the place along it besides. Without
    // it the point slides, so it is not too much.
    let system = Points {
        n: 2,
        constraints: vec![C::At(0, 0.0, 0.0), C::Level(0, 1), C::At(1, 2.0, 0.0)],
    };
    let x = vec![0.0, 0.0, 2.0, 0.0];
    let d = diagnose(&system, &x, TOLERANCE);
    assert_eq!(d.dof, 0);
    assert!(d.redundant.is_empty() && d.conflicting.is_empty(), "{d:?}");
}
