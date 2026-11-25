//! Splines through given places, held at their ends: a curve through
//! points, a loft through sections. What is interpolated is rows of
//! homogeneous control points (`[x w, y w, z w, w]`): one point for a
//! curve, a section's control points for a loft.

use anyhow::ensure;

/// How a spline is held at one of its ends: its first derivative there
/// and its second, where they are given (rows as long as the places').
#[derive(Debug, Clone, Default)]
pub struct Held {
    /// The first derivative along the spline's parameter.
    pub first: Option<Vec<[f64; 4]>>,
    /// The second.
    pub second: Option<Vec<[f64; 4]>>,
}

impl Held {
    /// How many conditions it adds.
    fn count(&self) -> usize {
        usize::from(self.first.is_some()) + usize::from(self.second.is_some())
    }
}

/// A spline: its degree, its knots, and its rows of control points.
pub type Spline = (usize, Vec<f64>, Vec<Vec<[f64; 4]>>);

/// The values of all the B-spline basis functions of `degree` over
/// `knots` at `t`, and of their first and second derivatives.
fn basis_all(degree: usize, knots: &[f64], t: f64) -> [Vec<f64>; 3] {
    let n = knots.len() - degree - 1;
    let last = knots[knots.len() - 1];
    // Degree 0: the span `t` is in (the last one that is not empty, at
    // the end).
    let mut values: Vec<f64> = (0..knots.len() - 1)
        .map(|i| {
            let (a, b) = (knots[i], knots[i + 1]);
            let inside = (a <= t && t < b) || (t >= last && b >= last && a < b);
            f64::from(u8::from(inside))
        })
        .collect();
    // (At the very end only one span counts.)
    if t >= last
        && let Some(first) = values.iter().position(|v| *v > 0.0)
    {
        for v in values.iter_mut().skip(first + 1) {
            *v = 0.0;
        }
    }
    let ratio = |x: f64, y: f64| if y == 0.0 { 0.0 } else { x / y };
    let mut by_degree = vec![values.clone()];
    for p in 1..=degree {
        let below = &by_degree[p - 1];
        let next: Vec<f64> = (0..knots.len() - 1 - p)
            .map(|i| {
                ratio(t - knots[i], knots[i + p] - knots[i]) * below[i]
                    + ratio(knots[i + p + 1] - t, knots[i + p + 1] - knots[i + 1]) * below[i + 1]
            })
            .collect();
        by_degree.push(next);
    }
    // N'_{i,p} = p (N_{i,p-1} / (t_{i+p} - t_i) - N_{i+1,p-1} / (t_{i+p+1} - t_{i+1})).
    let derived = |of: &dyn Fn(usize, usize) -> f64, p: usize, i: usize| -> f64 {
        if p == 0 {
            return 0.0;
        }
        p as f64
            * (ratio(of(p - 1, i), knots[i + p] - knots[i])
                - ratio(of(p - 1, i + 1), knots[i + p + 1] - knots[i + 1]))
    };
    let value = |p: usize, i: usize| by_degree[p].get(i).copied().unwrap_or(0.0);
    let first = |p: usize, i: usize| derived(&value, p, i);
    let second = |p: usize, i: usize| derived(&first, p, i);
    [
        (0..n).map(|i| value(degree, i)).collect(),
        (0..n).map(|i| first(degree, i)).collect(),
        (0..n).map(|i| second(degree, i)).collect(),
    ]
}

/// The spline (its degree, at most cubic, its knots and its rows of
/// control points) that is `rows[k]` at `params[k]` (rising, from 0 to
/// 1) and is held at its ends as `ends` say.
pub fn interpolate(
    params: &[f64],
    rows: Vec<Vec<[f64; 4]>>,
    ends: [&Held; 2],
) -> anyhow::Result<Spline> {
    let m = params.len();
    ensure!(
        m >= 2 && rows.len() == m,
        "a spline goes through two places or more"
    );
    ensure!(
        params.windows(2).all(|w| w[1] > w[0]),
        "two places after each other are one"
    );
    let (at_start, at_end) = (ends[0].count(), ends[1].count());
    let n = m + at_start + at_end;
    let degree = (n - 1).min(3);
    // The knots: averages of the places, those at an end counted once
    // more for each condition held there.
    let mut spread = vec![params[0]; 1 + at_start];
    spread.extend(&params[1..m - 1]);
    spread.extend(std::iter::repeat_n(params[m - 1], 1 + at_end));
    let mut knots = vec![params[0]; degree + 1];
    for j in 1..n - degree {
        knots.push(spread[j..j + degree].iter().sum::<f64>() / degree as f64);
    }
    knots.extend(std::iter::repeat_n(params[m - 1], degree + 1));
    let mut matrix = Vec::new();
    let mut wanted = Vec::new();
    for (k, (t, row)) in params.iter().zip(rows).enumerate() {
        matrix.push(basis_all(degree, &knots, *t)[0].clone());
        wanted.push(row);
        // The ends' conditions beside their places.
        let held = match k {
            0 => Some((ends[0], *t)),
            _ if k == m - 1 => Some((ends[1], *t)),
            _ => None,
        };
        if let Some((held, t)) = held {
            let all = basis_all(degree, &knots, t);
            for (order, given) in [(1, &held.first), (2, &held.second)] {
                if let Some(given) = given {
                    ensure!(
                        order <= degree,
                        "too few places for what the ends are held to"
                    );
                    matrix.push(all[order].clone());
                    wanted.push(given.clone());
                }
            }
        }
    }
    let control = crate::sweep::through(matrix, wanted)
        .ok_or_else(|| anyhow::anyhow!("the spline could not be fitted through the places"))?;
    Ok((degree, knots, control))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spline_goes_through_its_places_and_is_held_at_its_ends() {
        let params = [0.0, 0.3, 1.0];
        let rows: Vec<Vec<[f64; 4]>> = [[0.0, 0.0], [1.0, 2.0], [3.0, 1.0]]
            .iter()
            .map(|p| vec![[p[0], p[1], 0.0, 1.0]])
            .collect();
        let start = Held {
            first: Some(vec![[0.0, 5.0, 0.0, 0.0]]),
            second: Some(vec![[1.0, 0.0, 0.0, 0.0]]),
        };
        let end = Held {
            first: Some(vec![[4.0, 0.0, 0.0, 0.0]]),
            second: None,
        };
        let (degree, knots, control) = interpolate(&params, rows.clone(), [&start, &end]).unwrap();
        let at = |t: f64, order: usize| -> [f64; 4] {
            let b = &basis_all(degree, &knots, t)[order];
            let mut sum = [0.0; 4];
            for (v, row) in b.iter().zip(&control) {
                for i in 0..4 {
                    sum[i] += v * row[0][i];
                }
            }
            sum
        };
        for (t, row) in params.iter().zip(&rows) {
            let got = at(*t, 0);
            assert!((0..3).all(|i| (got[i] - row[0][i]).abs() < 1e-9), "{got:?}");
        }
        let d = at(0.0, 1);
        assert!((d[1] - 5.0).abs() < 1e-9 && d[0].abs() < 1e-9, "{d:?}");
        let dd = at(0.0, 2);
        assert!((dd[0] - 1.0).abs() < 1e-9 && dd[1].abs() < 1e-9, "{dd:?}");
        let e = at(1.0, 1);
        assert!((e[0] - 4.0).abs() < 1e-9 && e[1].abs() < 1e-9, "{e:?}");
    }
}
