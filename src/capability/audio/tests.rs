//! Resampling, band-limited: what the new rate cannot hold is removed rather than folded into the band, what it can is
//! kept, the same sound at two rates comes out the same, and the end of a signal resampled with its context is the end
//! of the whole signal resampled.

use std::f64::consts::TAU;

use super::{context, resample};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// One second of `tones` (frequency, amplitude) at `rate` Hz.
fn tones(rate: u32, tones: &[(f64, f64)]) -> Vec<f32> {
    (0..rate)
        .map(|i| {
            let t = f64::from(i) / f64::from(rate);
            tones
                .iter()
                .map(|(hz, a)| a * (TAU * hz * t).sin())
                .sum::<f64>() as f32
        })
        .collect()
}

/// The RMS of `samples` away from their ends, where the filter runs out of signal.
fn rms(samples: &[f32]) -> f64 {
    let inner = &samples[200..samples.len() - 200];
    (inner.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / inner.len() as f64).sqrt()
}

#[test]
fn what_the_new_rate_cannot_hold_is_removed_not_folded_into_the_band() {
    // 12 kHz is above 16 kHz's Nyquist frequency: linearly, 48 kHz to 16 kHz kept it whole, as a 4 kHz tone.
    for rate in [48_000, 44_100] {
        let down = resample(&tones(rate, &[(12_000.0, 1.0)]), rate, 16_000);
        assert_eq!(down.len(), 16_000);
        assert!(rms(&down) < 0.01, "{rate} Hz: RMS {}", rms(&down));
    }
}

#[test]
fn what_it_can_hold_is_kept() {
    let down = resample(&tones(48_000, &[(1_000.0, 1.0)]), 48_000, 16_000);
    let expected = tones(16_000, &[(1_000.0, 1.0)]);
    let worst = down[200..15_800]
        .iter()
        .zip(&expected[200..15_800])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(worst < 0.01, "{worst}");
    assert!((rms(&down) - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01);
}

#[test]
fn the_same_sound_at_two_rates_comes_out_the_same() {
    let sound = [
        (300.0, 0.4),
        (1_200.0, 0.3),
        (3_500.0, 0.2),
        (11_000.0, 0.3),
    ];
    let from_48 = resample(&tones(48_000, &sound), 48_000, 16_000);
    let from_44 = resample(&tones(44_100, &sound), 44_100, 16_000);
    let worst = from_48[200..15_800]
        .iter()
        .zip(&from_44[200..15_800])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(worst < 0.02, "{worst}");
}

#[test]
fn the_end_resampled_with_its_context_is_the_end_of_the_whole() {
    let whole = tones(48_000, &[(440.0, 0.5), (2_000.0, 0.3)]);
    let all = resample(&whole, 48_000, 16_000);
    // A whole number of output samples back, so that both grids fall on the same input samples.
    let kept = (24_000 + context(48_000, 16_000)).div_ceil(3) * 3;
    let tail = resample(&whole[whole.len() - kept..], 48_000, 16_000);
    let worst = all[all.len() - 8_000..]
        .iter()
        .zip(&tail[tail.len() - 8_000..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(worst < 1e-4, "{worst}");
    assert_eq!(context(16_000, 16_000), 0);
}

#[test]
fn a_level_holds_up_to_the_ends_and_the_same_rate_is_a_copy() {
    let up = resample(&[0.5; 100], 8_000, 16_000);
    assert_eq!(up.len(), 200);
    assert!(up.iter().all(|sample| (sample - 0.5).abs() < 1e-6));
    assert_eq!(resample(&[0.1, 0.2], 16_000, 16_000), [0.1, 0.2]);
    assert_eq!(resample(&[0.0; 48_000], 48_000, 16_000).len(), 16_000);
}
