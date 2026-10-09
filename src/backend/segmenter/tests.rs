//! The segmenter on made-up probabilities, at 16 kHz in windows of 512 samples seen with 64 of context, as Silero v5
//! runs: when speech is confirmed and when it ends, where its segment starts and ends, and what short blips and dips
//! do.

use std::ops::Range;

use super::Segmenter;
use crate::capability::VadOptions;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

const WINDOW: u64 = 512;

fn segmenter() -> Segmenter {
    let options = VadOptions {
        threshold: 0.5,
        min_silence_ms: 500,
        min_speech_ms: 250,
    };
    Segmenter::new(&options, 16_000, 512, 64)
}

/// `runs` of (windows, probability), in order: whether each window is in speech, and the segments that ended.
fn run(segmenter: &mut Segmenter, runs: &[(usize, f32)]) -> (Vec<bool>, Vec<Range<u64>>) {
    let mut speech = Vec::new();
    let mut ended = Vec::new();
    for &(windows, probability) in runs {
        for _ in 0..windows {
            let window = segmenter.window(probability);
            assert_eq!(window.probability, Some(probability));
            speech.push(window.speech);
            ended.extend(window.ended);
        }
    }
    (speech, ended)
}

#[test]
fn speech_is_confirmed_after_min_speech_and_ends_after_min_silence() {
    let mut segmenter = segmenter();
    let (speech, ended) = run(&mut segmenter, &[(20, 0.0), (30, 0.9), (30, 0.0)]);
    // Speech begins at window 21 (sample 10 240); 250 ms (4 000 samples) later, window 29 confirms it.
    let first = speech.iter().position(|speech| *speech).expect("speech");
    assert_eq!(first, 28);
    // It stops at window 51; 500 ms (8 000 samples) of silence later, window 67 ends it.
    let last = speech.iter().rposition(|speech| *speech).expect("speech");
    assert_eq!(last, 65);
    // Starting two context windows (1 152) and min_speech (4 000) before window 29's end (14 848), and ending min_silence
    // before window 67's end (34 304).
    assert_eq!(ended, vec![9_696..26_304]);
    assert!(ended[0].start < 20 * WINDOW && ended[0].end > 50 * WINDOW);
}

#[test]
fn a_blip_shorter_than_min_speech_is_not_speech() {
    let mut segmenter = segmenter();
    let (speech, ended) = run(&mut segmenter, &[(10, 0.0), (5, 0.9), (40, 0.0)]);
    assert!(speech.iter().all(|speech| !speech));
    assert!(ended.is_empty());
}

#[test]
fn a_dip_shorter_than_min_silence_or_above_the_lower_threshold_keeps_the_speech() {
    let mut segmenter = segmenter();
    let (speech, ended) = run(
        &mut segmenter,
        &[
            (10, 0.0),
            (20, 0.9),
            (10, 0.1),
            (20, 0.9),
            (30, 0.4),
            (20, 0.9),
            (30, 0.0),
        ],
    );
    assert_eq!(ended.len(), 1, "one segment: {ended:?}");
    let first = speech.iter().position(|speech| *speech).expect("speech");
    let last = speech.iter().rposition(|speech| *speech).expect("speech");
    assert!(speech[first..=last].iter().all(|speech| *speech));
}

#[test]
fn a_segment_never_starts_before_the_previous_one_ended() {
    let mut segmenter = segmenter();
    let (_, ended) = run(
        &mut segmenter,
        &[(10, 0.0), (20, 0.9), (20, 0.0), (20, 0.9), (30, 0.0)],
    );
    assert_eq!(ended.len(), 2, "{ended:?}");
    assert!(ended[1].start >= ended[0].end, "{ended:?}");
}

#[test]
fn finishing_ends_the_speech_at_the_last_sample_and_starts_over() {
    let mut segmenter = segmenter();
    let (speech, ended) = run(&mut segmenter, &[(10, 0.0), (20, 0.9)]);
    assert!(speech.last().copied().unwrap_or_default() && ended.is_empty());
    let finished = segmenter.finish().expect("speech in progress");
    assert_eq!(finished.end, 30 * WINDOW);
    assert_eq!(segmenter.finish(), None, "nothing in progress any more");
    let (speech, _) = run(&mut segmenter, &[(1, 0.9)]);
    assert_eq!(speech, [false], "the state started over");
}

#[test]
fn reset_forgets_speech_in_progress() {
    let mut segmenter = segmenter();
    run(&mut segmenter, &[(10, 0.0), (20, 0.9)]);
    segmenter.reset();
    assert_eq!(segmenter.finish(), None);
}
