//! FIR filtering of long signals by overlap-add FFT convolution, returning the
//! centred part the length of the input, like SciPy's `fftconvolve(mode="same")`.

use realfft::RealFftPlanner;

pub fn same(signal: &[f32], kernel: &[f64]) -> Vec<f32> {
    let n = signal.len();
    let m = kernel.len();
    if n == 0 || m == 0 {
        return vec![0.0; n];
    }
    let block = (4 * m).next_power_of_two().max(8192);
    let step = block - m + 1;
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(block);
    let inverse = planner.plan_fft_inverse(block);

    let mut kernel_time = vec![0.0; block];
    kernel_time[..m].copy_from_slice(kernel);
    let mut kernel_freq = forward.make_output_vec();
    forward.process(&mut kernel_time, &mut kernel_freq).expect("kernel transform");

    let full = n + m - 1;
    let mut out = vec![0.0f64; full + block];
    let mut time = forward.make_input_vec();
    let mut freq = forward.make_output_vec();
    let mut back = inverse.make_output_vec();
    let scale = 1.0 / block as f64;

    let mut start = 0;
    while start < n {
        let end = (start + step).min(n);
        time.iter_mut().for_each(|v| *v = 0.0);
        for (slot, &sample) in time.iter_mut().zip(&signal[start..end]) {
            *slot = sample as f64;
        }
        forward.process(&mut time, &mut freq).expect("block transform");
        for (value, k) in freq.iter_mut().zip(&kernel_freq) {
            *value *= *k;
        }
        inverse.process(&mut freq, &mut back).expect("block inverse");
        for (i, v) in back.iter().enumerate() {
            out[start + i] += v * scale;
        }
        start = end;
    }

    let offset = (m - 1) / 2;
    out[offset..offset + n].iter().map(|&v| v as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_direct_convolution() {
        let signal: Vec<f32> = (0..20_000).map(|i| ((i as f32) * 0.013).sin() + ((i * 7 % 13) as f32 - 6.0) * 0.01).collect();
        let kernel: Vec<f64> = (0..101).map(|i| ((i as f64 - 50.0) * 0.1).exp().recip().min(1.0) * 0.05).collect();
        let fast = same(&signal, &kernel);
        let offset = (kernel.len() - 1) / 2;
        for i in [0usize, 7, 5000, 12_345, 19_999] {
            let mut direct = 0.0f64;
            for (j, k) in kernel.iter().enumerate() {
                let idx = i as isize + offset as isize - j as isize;
                if idx >= 0 && (idx as usize) < signal.len() {
                    direct += signal[idx as usize] as f64 * k;
                }
            }
            assert!((fast[i] as f64 - direct).abs() < 1e-4, "at {i}: {} vs {direct}", fast[i]);
        }
    }
}
