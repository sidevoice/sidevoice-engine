//! The features against Whisper's own extractor: values transformers' `WhisperFeatureExtractor(chunk_length=8)` gives
//! for a made-up signal (two tones, a chirp and a quiet noise, 1.5 s), padded as smart-turn's inference pads it.

use super::{features, FRAMES, MELS};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// The signal, as the reference was made from it.
fn signal() -> Vec<f32> {
    let mut seed: u64 = 12_345;
    (0..24_000)
        .map(|i| {
            seed = (1_103_515_245 * seed + 12_345) % (1 << 31);
            let noise = seed as f64 / f64::from(1u32 << 31) - 0.5;
            let t = f64::from(i) / 16_000.0;
            let tau = 2.0 * std::f64::consts::PI;
            (0.3 * (tau * 220.0 * t).sin()
                + 0.2 * (tau * 1375.0 * t).sin()
                + 0.1 * (tau * (300.0 + 2000.0 * t) * t).sin()
                + 0.05 * noise) as f32
        })
        .collect()
}

#[test]
fn features_match_whispers_extractor() {
    let features = features(&signal());
    assert_eq!(features.len(), MELS * FRAMES);
    let at = |mel: usize, frame: usize| features[mel * FRAMES + frame];
    // [mel, frame, value] from transformers 4.x, numpy float32.
    let reference = [
        (0, 0, -0.225_363_25),
        (0, 799, 1.001_676_3),
        (10, 700, 0.664_104_46),
        (40, 400, -0.225_363_25),
        (79, 799, 0.679_649_6),
        (20, 795, 0.704_562_2),
        (60, 650, 0.652_382_6),
        (5, 100, -0.225_363_25),
    ];
    for (mel, frame, expected) in reference {
        let got = at(mel, frame);
        assert!(
            (got - expected).abs() < 2e-3,
            "[{mel}, {frame}]: {got}, not {expected}"
        );
    }
    let mean = features.iter().map(|x| f64::from(*x)).sum::<f64>() / features.len() as f64;
    assert!((mean - -0.031_271_66).abs() < 1e-3, "mean {mean}");
    let max = features.iter().copied().fold(f32::MIN, f32::max);
    assert!((max - 1.774_636_7).abs() < 2e-3, "max {max}");
}

#[test]
fn a_long_turn_keeps_its_end_and_silence_is_flat() {
    let long: Vec<f32> = [vec![0.25; 16_000 * 10], signal()].concat();
    // Only the last 8 s count.
    assert_eq!(features(&long), features(&long[long.len() - 128_000..]));
    let silence = features(&[]);
    assert!(silence.iter().all(|x| (x - silence[0]).abs() < 1e-6));
}

/// Natively, a chunk at a time gives the same features: nothing waits between chunks.
#[cfg(native)]
#[test]
fn chunked_extraction_gives_the_same_features() {
    let chunked = crate::test_support::ready(super::features_yielding(&signal()));
    assert_eq!(chunked, features(&signal()));
}

/// In a page, extraction lets the page run between chunks: a task the page queued while it ran gets to run before it
/// ends, once per chunk, rather than wait for the whole extraction (which would block the page as long as it lasts).
#[cfg(web)]
mod in_a_page {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::{features, signal};
    use crate::backend::smart_turn::{features_yielding, CHUNK, FRAMES};

    #[wasm_bindgen(inline_js = r#"
export function startTicker() {
  const ticker = { ticks: 0, running: true };
  const channel = new MessageChannel();
  channel.port1.onmessage = () => {
    if (!ticker.running) { channel.port1.close(); return; }
    ticker.ticks += 1;
    channel.port2.postMessage(null);
  };
  channel.port2.postMessage(null);
  return ticker;
}
export function stopTicker(ticker) { ticker.running = false; return ticker.ticks; }
"#)]
    extern "C" {
        #[wasm_bindgen(js_name = startTicker)]
        fn start_ticker() -> JsValue;
        #[wasm_bindgen(js_name = stopTicker)]
        fn stop_ticker(ticker: &JsValue) -> u32;
    }

    #[wasm_bindgen_test]
    async fn the_page_runs_between_chunks_and_the_features_are_the_same() {
        let ticker = start_ticker();
        let chunked = features_yielding(&signal()).await;
        let ticks = stop_ticker(&ticker);
        let chunks = FRAMES.div_ceil(CHUNK) as u32;
        assert!(
            ticks + 1 >= chunks,
            "the page ran {ticks} times during {chunks} chunks"
        );
        assert_eq!(chunked, features(&signal()));
    }
}
