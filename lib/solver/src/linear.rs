//! The linear algebra: a Cholesky factorization of a positive
//! semidefinite matrix that skips dependent rows.

/// Pivots below this fraction of the largest diagonal entry count as
/// zero: the row depends on earlier ones.
const RANK: f64 = 1e-11;

/// `L Lᵀ` of the rows of a positive semidefinite matrix that are
/// independent of the rows before them.
pub(crate) struct Factor {
    m: usize,
    /// Row-major lower triangle; rows and columns of dropped rows are
    /// zero.
    l: Vec<f64>,
    /// Whether each row was kept.
    kept: Vec<bool>,
    /// The rows that depend on earlier ones, in order.
    pub(crate) dropped: Vec<usize>,
}

impl Factor {
    /// Factors the `m`×`m` matrix `a` (row-major, symmetric), in order:
    /// a row that depends on the rows before it is dropped.
    pub(crate) fn new(a: Vec<f64>, m: usize) -> Self {
        let scale = (0..m).map(|i| a[i * m + i]).fold(0.0, f64::max);
        let mut l = vec![0.0; m * m];
        let mut kept = vec![false; m];
        let mut dropped = Vec::new();
        for i in 0..m {
            for j in 0..=i {
                if j < i && !kept[j] {
                    continue;
                }
                let mut sum = a[i * m + j];
                for k in 0..j {
                    sum -= l[i * m + k] * l[j * m + k];
                }
                if j < i {
                    l[i * m + j] = sum / l[j * m + j];
                } else if sum > RANK * scale && sum > 0.0 {
                    l[i * m + i] = sum.sqrt();
                    kept[i] = true;
                } else {
                    // Dependent: forget the row.
                    for k in 0..i {
                        l[i * m + k] = 0.0;
                    }
                    dropped.push(i);
                }
            }
        }
        Self {
            m,
            l,
            kept,
            dropped,
        }
    }

    /// How many rows are independent.
    pub(crate) fn rank(&self) -> usize {
        self.m - self.dropped.len()
    }

    /// `L⁻¹ b` over the kept rows (zero for dropped ones).
    pub(crate) fn forward(&self, b: &[f64]) -> Vec<f64> {
        let m = self.m;
        let mut y = vec![0.0; m];
        for i in 0..m {
            if !self.kept[i] {
                continue;
            }
            let ahead: f64 = (0..i).map(|k| self.l[i * m + k] * y[k]).sum();
            let sum = b[i] - ahead;
            y[i] = sum / self.l[i * m + i];
        }
        y
    }

    /// The solution of `A x = b` over the kept rows, zero for dropped
    /// ones.
    pub(crate) fn solve(&self, b: &[f64]) -> Vec<f64> {
        let m = self.m;
        let mut x = self.forward(b);
        for i in (0..m).rev() {
            if !self.kept[i] {
                continue;
            }
            let behind: f64 = (i + 1..m).map(|k| self.l[k * m + i] * x[k]).sum();
            let sum = x[i] - behind;
            x[i] = sum / self.l[i * m + i];
        }
        x
    }
}
