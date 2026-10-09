//! Audio as the engine hands it over and takes it: mono samples and their rate. Speech to text and end of turn take
//! any rate and the engine brings it to the model's, band-limited; text to speech returns the model's own rate.

#[cfg(test)]
mod tests;

/// Mono samples, in [-1, 1], at `sample_rate` Hz.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    /// The samples.
    pub samples: Vec<f32>,
    /// Their rate, in Hz.
    pub sample_rate: u32,
}

/// `samples` at `from` Hz, at `to` Hz, band-limited: a windowed-sinc low-pass (a Blackman window over [`ZEROS`] zero
/// crossings each side) whose cutoff sits just under the lower rate's Nyquist frequency. Downsampling so removes what
/// the new rate cannot hold rather than fold it into the band (48 kHz to 16 kHz: a 12 kHz tone does not come back as
/// 4 kHz); upsampling interpolates. Each output is normalized by the weights it used, so the ends, where the window
/// runs past the samples, keep their level.
pub(crate) fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let ratio = f64::from(to) / f64::from(from);
    let cutoff = ratio.min(1.0) * PASSBAND;
    let half = f64::from(ZEROS) / cutoff;
    let len = (samples.len() as f64 * ratio) as usize;
    (0..len)
        .map(|i| {
            let at = i as f64 / ratio;
            let first = (at - half).ceil().max(0.0) as usize;
            let last = ((at + half).floor() as usize).min(samples.len() - 1);
            let (mut sum, mut weights) = (0.0f64, 0.0f64);
            for (k, sample) in samples.iter().enumerate().take(last + 1).skip(first) {
                let distance = at - k as f64;
                let weight = cutoff * sinc(cutoff * distance) * blackman(distance / half);
                sum += f64::from(*sample) * weight;
                weights += weight;
            }
            if weights.abs() < f64::EPSILON {
                0.0
            } else {
                (sum / weights) as f32
            }
        })
        .collect()
}

/// How many input samples, at `from` Hz, the filter reaches on each side of an output sample when resampling to `to`
/// Hz: what a caller that resamples only the end of a signal keeps before it, so that its first kept samples see their
/// past.
pub(crate) fn context(from: u32, to: u32) -> usize {
    if from == to || from == 0 || to == 0 {
        return 0;
    }
    let ratio = f64::from(to) / f64::from(from);
    (f64::from(ZEROS) / (ratio.min(1.0) * PASSBAND)).ceil() as usize + 1
}

/// The filter's zero crossings on each side: its length, and how sharply it cuts.
const ZEROS: u32 = 16;
/// Where it cuts, as a fraction of the lower rate's Nyquist frequency: just under it, so that its transition band ends
/// near Nyquist rather than straddle it.
const PASSBAND: f64 = 0.9;

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        let x = std::f64::consts::PI * x;
        x.sin() / x
    }
}

/// The Blackman window at `x` in [-1, 1], 0 outside.
fn blackman(x: f64) -> f64 {
    if x.abs() >= 1.0 {
        return 0.0;
    }
    let angle = std::f64::consts::PI * (x + 1.0);
    0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos()
}
