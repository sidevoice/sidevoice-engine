//! Audio as the engine hands it over and takes it: mono samples and their rate. Speech to text takes any rate and the
//! engine brings it to the model's; text to speech returns the model's own rate.

/// Mono samples, in [-1, 1], at `sample_rate` Hz.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// The samples.
    pub samples: Vec<f32>,
    /// Their rate, in Hz.
    pub sample_rate: u32,
}

/// `samples` at `from` Hz, linearly resampled to `to` Hz: enough for speech recognition, which is what it is for.
pub(crate) fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let step = f64::from(from) / f64::from(to);
    let len = (samples.len() as f64 / step) as usize;
    (0..len)
        .map(|i| {
            let at = i as f64 * step;
            let (index, frac) = (at as usize, at.fract() as f32);
            let here = samples[index.min(samples.len() - 1)];
            let next = samples.get(index + 1).copied().unwrap_or(here);
            here + (next - here) * frac
        })
        .collect()
}
