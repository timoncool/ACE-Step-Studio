//! LOWESS smoothing, the local linear regression of statsmodels'
//! `nonparametric.lowess` without robustness passes (`it = 0`), including its
//! `delta` shortcut of interpolating between regressions closer than `delta`.

/// Smooths `y` sampled at `x` (increasing). Returns the fitted values.
pub fn smooth(x: &[f64], y: &[f64], frac: f64, delta: f64) -> Vec<f64> {
    let n = x.len();
    let k = ((frac * n as f64 + 1e-10) as usize).clamp(2, n);
    let mut fit = vec![0.0; n];
    let mut weights = vec![0.0; n];
    let (mut left, mut right) = (0usize, k);
    let mut i = 0usize;
    let mut last_fit: isize = -1;

    loop {
        let xval = x[i];
        // slide the k-nearest-neighbour window
        while right < n && xval > (x[left] + x[right]) / 2.0 {
            left += 1;
            right += 1;
        }
        let radius = (xval - x[left]).max(x[right - 1] - xval);

        // tricube weights, normalised
        let mut sum = 0.0;
        let mut nonzero = 0;
        for j in left..right {
            let d = if radius > 0.0 { ((x[j] - xval).abs() / radius).min(1.0) } else { 0.0 };
            let w = (1.0 - d * d * d).powi(3);
            weights[j] = w;
            sum += w;
            if w > 1e-12 {
                nonzero += 1;
            }
        }
        if nonzero < 2 {
            fit[i] = y[i];
        } else {
            let mut mean_x = 0.0;
            for j in left..right {
                weights[j] /= sum;
                mean_x += weights[j] * x[j];
            }
            let mut spread = 0.0;
            for j in left..right {
                spread += weights[j] * (x[j] - mean_x).powi(2);
            }
            let spread = spread.max(1e-12);
            let mut value = 0.0;
            for j in left..right {
                let p = weights[j] * (1.0 + (xval - mean_x) * (x[j] - mean_x) / spread);
                value += p * y[j];
            }
            fit[i] = value;
        }

        // linear interpolation over the points skipped since the last fit
        if last_fit < i as isize - 1 {
            let from = last_fit as usize;
            let span = x[i] - x[from];
            for j in from + 1..i {
                let a = (x[j] - x[from]) / span;
                fit[j] = a * fit[i] + (1.0 - a) * fit[from];
            }
        }

        // next regression point: the farthest one within delta
        let mut last = i;
        let cut = x[last] + delta;
        let mut k_next = last;
        for j in last + 1..n {
            k_next = j;
            if x[j] > cut {
                break;
            }
            if x[j] == x[last] {
                fit[j] = fit[last];
                last = j;
            }
        }
        last_fit = last as isize;
        if last_fit as usize >= n - 1 {
            break;
        }
        i = (k_next.saturating_sub(1)).max(last + 1);
    }
    fit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_stays_a_line() {
        let x: Vec<f64> = (0..400).map(|i| i as f64 / 399.0).collect();
        let y: Vec<f64> = x.iter().map(|&v| 3.0 * v - 1.0).collect();
        let fit = smooth(&x, &y, 0.0375, 0.001);
        for (a, b) in fit.iter().zip(&y) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn noise_is_smoothed_around_the_trend() {
        let x: Vec<f64> = (0..2000).map(|i| i as f64 / 1999.0).collect();
        let y: Vec<f64> = x.iter().enumerate().map(|(i, &v)| v + if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
        let fit = smooth(&x, &y, 0.05, 0.0);
        let worst = fit.iter().zip(&x).skip(100).take(1800).map(|(f, v)| (f - v).abs()).fold(0.0, f64::max);
        assert!(worst < 0.01, "{worst}");
    }
}
