//! Cubic spline interpolation with not-a-knot ends, the curve SciPy's
//! `interp1d(kind="cubic")` draws, extrapolated with the end polynomials.

pub struct CubicSpline {
    x: Vec<f64>,
    y: Vec<f64>,
    m: Vec<f64>, // second derivatives at the knots
}

impl CubicSpline {
    /// `x` strictly increasing, at least four points.
    pub fn new(x: &[f64], y: &[f64]) -> Self {
        let n = x.len();
        assert!(n >= 4 && y.len() == n, "a not-a-knot spline needs four points or more");
        let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
        let slope = |i: usize| (y[i + 1] - y[i]) / h[i];

        // Unknowns M1..M(n-2); the not-a-knot conditions fold M0 and M(n-1)
        // into the first and last rows:
        //   M0     = ((h0 + h1) M1 - h0 M2) / h1
        //   M(n-1) = ((h(n-3) + h(n-2)) M(n-2) - h(n-2) M(n-3)) / h(n-3)
        let size = n - 2;
        let mut lower = vec![0.0; size];
        let mut diag = vec![0.0; size];
        let mut upper = vec![0.0; size];
        let mut rhs = vec![0.0; size];
        for row in 0..size {
            let i = row + 1;
            lower[row] = h[i - 1];
            diag[row] = 2.0 * (h[i - 1] + h[i]);
            upper[row] = h[i];
            rhs[row] = 6.0 * (slope(i) - slope(i - 1));
        }
        // first row: h0 M0 + 2(h0+h1) M1 + h1 M2, with M0 substituted
        let (h0, h1) = (h[0], h[1]);
        diag[0] += h0 * (h0 + h1) / h1;
        upper[0] -= h0 * h0 / h1;
        lower[0] = 0.0;
        // last row, symmetric
        let (ha, hb) = (h[n - 3], h[n - 2]);
        let last = size - 1;
        if size == 1 {
            // n == 3 cannot happen (n >= 4), kept for clarity
            diag[last] += hb * (ha + hb) / ha;
        } else {
            diag[last] += hb * (ha + hb) / ha;
            lower[last] -= hb * hb / ha;
            upper[last] = 0.0;
        }

        // Thomas algorithm
        let mut c = vec![0.0; size];
        let mut d = vec![0.0; size];
        c[0] = upper[0] / diag[0];
        d[0] = rhs[0] / diag[0];
        for i in 1..size {
            let denom = diag[i] - lower[i] * c[i - 1];
            c[i] = if i + 1 < size { upper[i] / denom } else { 0.0 };
            d[i] = (rhs[i] - lower[i] * d[i - 1]) / denom;
        }
        let mut inner = vec![0.0; size];
        inner[size - 1] = d[size - 1];
        for i in (0..size - 1).rev() {
            inner[i] = d[i] - c[i] * inner[i + 1];
        }

        let mut m = vec![0.0; n];
        m[1..n - 1].copy_from_slice(&inner);
        m[0] = ((h0 + h1) * m[1] - h0 * m[2]) / h1;
        m[n - 1] = ((ha + hb) * m[n - 2] - hb * m[n - 3]) / ha;
        Self { x: x.to_vec(), y: y.to_vec(), m }
    }

    pub fn at(&self, t: f64) -> f64 {
        let n = self.x.len();
        // interval: clamp to the end ones outside the range, which extrapolates
        let i = match self.x.binary_search_by(|v| v.partial_cmp(&t).unwrap()) {
            Ok(i) => i.min(n - 2),
            Err(0) => 0,
            Err(i) => (i - 1).min(n - 2),
        };
        let (x0, x1) = (self.x[i], self.x[i + 1]);
        let h = x1 - x0;
        let a = (x1 - t) / h;
        let b = (t - x0) / h;
        a * self.y[i]
            + b * self.y[i + 1]
            + ((a * a * a - a) * self.m[i] + (b * b * b - b) * self.m[i + 1]) * h * h / 6.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cubic_is_reproduced_exactly() {
        // not-a-knot splines interpolate any cubic without error
        let f = |x: f64| 2.0 * x * x * x - x * x + 3.0 * x - 1.0;
        let x: Vec<f64> = (0..9).map(|i| i as f64 * 0.7 + (i * i) as f64 * 0.05).collect();
        let y: Vec<f64> = x.iter().map(|&v| f(v)).collect();
        let spline = CubicSpline::new(&x, &y);
        for t in [0.1, 1.3, 2.9, 4.4, 6.9, -0.5, 8.0] {
            assert!((spline.at(t) - f(t)).abs() < 1e-8, "at {t}: {} vs {}", spline.at(t), f(t));
        }
    }
}
