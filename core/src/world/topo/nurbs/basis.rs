//! B-spline basis functions (Piegl & Tiller, *The NURBS Book*, ch. 2).

/// The knot span containing `t` (Piegl & Tiller A2.1), `t` clamped to the domain.
pub(super) fn find_span(degree: usize, knots: &[f64], n_points: usize, t: f64) -> usize {
    let n = n_points - 1;
    if t >= knots[n + 1] {
        // Last non-empty span.
        let mut s = n;
        while knots[s] >= knots[s + 1] {
            s -= 1;
        }
        return s;
    }
    if t <= knots[degree] {
        let mut s = degree;
        while knots[s] >= knots[s + 1] {
            s += 1;
        }
        return s;
    }
    let (mut lo, mut hi) = (degree, n + 1);
    let mut mid = (lo + hi) / 2;
    while t < knots[mid] || t >= knots[mid + 1] {
        if t < knots[mid] {
            hi = mid
        } else {
            lo = mid
        }
        mid = (lo + hi) / 2;
    }
    mid
}

/// How many numbers a [`Row`] holds without asking for memory: enough
/// for a degree of fifteen.
pub(super) const HELD: usize = 16;

/// A few numbers, one for each basis function that is not zero: held
/// in place for the degrees there are in practice (what evaluates
/// curves and surfaces is called millions of times, and should not ask
/// for memory each time).
pub(super) struct Row {
    held: [f64; HELD],
    more: Vec<f64>,
    len: usize,
}

impl Row {
    fn new(len: usize) -> Self {
        Self {
            held: [0.0; HELD],
            more: if len > HELD {
                vec![0.0; len]
            } else {
                Vec::new()
            },
            len,
        }
    }
}

impl std::ops::Deref for Row {
    type Target = [f64];

    fn deref(&self) -> &[f64] {
        if self.len > HELD {
            &self.more
        } else {
            &self.held[..self.len]
        }
    }
}

impl std::ops::DerefMut for Row {
    fn deref_mut(&mut self) -> &mut [f64] {
        if self.len > HELD {
            &mut self.more
        } else {
            &mut self.held[..self.len]
        }
    }
}

/// Fills `n` with the `degree + 1` non-zero basis functions at `t` in
/// `span` (A2.2), and `lower` (if asked for) with the `degree` of one
/// degree less: what the functions are a step before they are done.
/// `left` and `right` are room to work in, `degree + 1` long.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn fill(
    degree: usize,
    knots: &[f64],
    span: usize,
    t: f64,
    n: &mut [f64],
    mut lower: Option<&mut [f64]>,
    left: &mut [f64],
    right: &mut [f64],
) {
    n[0] = 1.0;
    if degree == 1
        && let Some(lower) = lower.as_deref_mut()
    {
        lower[0] = 1.0;
    }
    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            let tmp = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        n[j] = saved;
        if j + 1 == degree
            && let Some(lower) = lower.as_deref_mut()
        {
            lower[..degree].copy_from_slice(&n[..degree]);
        }
    }
}

/// What [`basis`] gives, written into `n`: for a degree below
/// [`HELD`], the case there is in practice, with nothing moved about.
#[inline(always)]
pub(super) fn basis_held(degree: usize, knots: &[f64], span: usize, t: f64, n: &mut [f64; HELD]) {
    debug_assert!(degree < HELD);
    let (mut left, mut right) = ([0.0; HELD], [0.0; HELD]);
    fill(degree, knots, span, t, n, None, &mut left, &mut right);
}

/// What [`basis_derivatives`] gives, written into `n` and `d`: for a
/// degree below [`HELD`].
#[inline(always)]
pub(super) fn derivatives_held(
    degree: usize,
    knots: &[f64],
    span: usize,
    t: f64,
    n: &mut [f64; HELD],
    d: &mut [f64; HELD],
) {
    debug_assert!(degree < HELD);
    let (mut left, mut right, mut lower) = ([0.0; HELD], [0.0; HELD], [0.0; HELD]);
    fill(
        degree,
        knots,
        span,
        t,
        n,
        Some(&mut lower),
        &mut left,
        &mut right,
    );
    let p = degree as f64;
    for j in 0..=degree {
        let i = span - degree + j;
        let mut x = 0.0;
        if j >= 1 {
            x += p * lower[j - 1] / (knots[i + degree] - knots[i]);
        }
        if j < degree {
            x -= p * lower[j] / (knots[i + degree + 1] - knots[i + 1]);
        }
        d[j] = x;
    }
}

/// The `degree + 1` non-zero basis functions at `t` in `span` (A2.2).
#[inline]
pub(super) fn basis(degree: usize, knots: &[f64], span: usize, t: f64) -> Row {
    let mut n = Row::new(degree + 1);
    if degree < HELD {
        let (mut left, mut right) = ([0.0; HELD], [0.0; HELD]);
        fill(
            degree,
            knots,
            span,
            t,
            &mut n.held,
            None,
            &mut left,
            &mut right,
        );
    } else {
        let (mut left, mut right) = (vec![0.0; degree + 1], vec![0.0; degree + 1]);
        fill(degree, knots, span, t, &mut n, None, &mut left, &mut right);
    }
    n
}

/// The non-zero basis functions at `t` in `span` and their first
/// derivatives.
#[inline]
pub(super) fn basis_derivatives(degree: usize, knots: &[f64], span: usize, t: f64) -> (Row, Row) {
    let mut n = Row::new(degree + 1);
    let mut d = Row::new(degree + 1);
    // N'ᵢ,ₚ = p·Nᵢ,ₚ₋₁ / (uᵢ₊ₚ − uᵢ) − p·Nᵢ₊₁,ₚ₋₁ / (uᵢ₊ₚ₊₁ − uᵢ₊₁).
    let p = degree as f64;
    if degree < HELD {
        let (mut left, mut right, mut lower) = ([0.0; HELD], [0.0; HELD], [0.0; HELD]);
        fill(
            degree,
            knots,
            span,
            t,
            &mut n.held,
            Some(&mut lower),
            &mut left,
            &mut right,
        );
        let d = &mut d.held;
        for j in 0..=degree {
            let i = span - degree + j;
            if j >= 1 {
                d[j] += p * lower[j - 1] / (knots[i + degree] - knots[i]);
            }
            if j < degree {
                d[j] -= p * lower[j] / (knots[i + degree + 1] - knots[i + 1]);
            }
        }
    } else {
        let mut lower = vec![0.0; degree + 1];
        let (mut left, mut right) = (vec![0.0; degree + 1], vec![0.0; degree + 1]);
        fill(
            degree,
            knots,
            span,
            t,
            &mut n,
            Some(&mut lower),
            &mut left,
            &mut right,
        );
        for j in 0..=degree {
            let i = span - degree + j;
            if j >= 1 {
                d[j] += p * lower[j - 1] / (knots[i + degree] - knots[i]);
            }
            if j < degree {
                d[j] -= p * lower[j] / (knots[i + degree + 1] - knots[i + 1]);
            }
        }
    }
    (n, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_is_a_partition_of_unity() {
        let knots = [0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let span = find_span(2, &knots, 4, t);
            let sum: f64 = basis(2, &knots, span, t).iter().sum();
            assert!((sum - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn span_skips_empty_spans_at_the_ends() {
        let knots = [0.0, 0.0, 1.0, 2.0, 2.0];
        assert_eq!(find_span(1, &knots, 3, 0.0), 1);
        assert_eq!(find_span(1, &knots, 3, 2.0), 2);
        assert_eq!(find_span(1, &knots, 3, 1.5), 2);
    }
}
