//! The loop's arithmetic and its plan, without running a model: word error rates, WAV files, resampling, how a
//! detection is judged, and that the plan names builds the bundled catalogue has. These run with every `cargo test`.

use std::fs;

use sidevoice_engine::{BundledCatalog, CatalogSource, ModelEntry};

use super::audio::{read_wav, resample, wav};
use super::end_of_turn::{self, EndOfTurnPlan};
use super::vad::{judge, padded, VadPlan};
use super::wer::{normalised, wer};
use super::{plan, plan_path, primary};

#[test]
fn texts_are_compared_without_case_punctuation_or_vowel_accents() {
    assert_eq!(normalised("¿Así que, MAÑANA?"), "asi que mañana");
    assert_eq!(normalised("  The quick-brown fox. "), "the quick brown fox");
}

#[test]
fn the_word_error_rate_counts_the_fewest_edits_over_the_words_said() {
    assert_eq!(wer("a b c d", "a b c d"), 0.0);
    assert_eq!(wer("a b c d", "a x c d"), 0.25, "one substitution");
    assert_eq!(wer("a b c d", "a c d"), 0.25, "one deletion");
    assert_eq!(wer("a b c d", "a b b c d"), 0.25, "one insertion");
    assert_eq!(wer("a b", ""), 1.0);
    assert_eq!(wer("a b", "x y z"), 1.5, "it can exceed 1");
    assert_eq!(wer("Hoy, así.", "hoy asi"), 0.0);
}

#[test]
fn a_wav_written_reads_back() {
    let samples: Vec<f32> = (0..2_400).map(|i| (i as f32 / 10.0).sin() * 0.5).collect();
    let (read, rate) = read_wav(&wav(&samples, 24_000)).expect("a WAV");
    assert_eq!(rate, 24_000);
    assert_eq!(read.len(), samples.len());
    assert!(read.iter().zip(&samples).all(|(a, b)| (a - b).abs() < 1e-3));
    assert!(read_wav(b"RIFF....WAVE").is_err());
}

#[test]
fn the_plan_names_bundled_builds_and_has_a_sentence_for_each_language() {
    let plan = plan(&fs::read_to_string(plan_path()).expect("voice_loop.json")).expect("a plan");
    let catalogue = BundledCatalog.load().expect("the bundled catalogue");
    let models: Vec<&ModelEntry> = catalogue
        .families
        .iter()
        .flat_map(|family| &family.models)
        .collect();
    let bundled = |id: &str| {
        models
            .iter()
            .any(|model| model.builds.iter().any(|build| build.id == id))
    };
    for speaker in &plan.tts {
        assert!(bundled(&speaker.build), "{}", speaker.build);
        assert!(!speaker.voices.is_empty(), "{}", speaker.build);
    }
    for (language, listeners) in &plan.stt {
        assert!(plan.sentences.contains_key(language), "{language}");
        for listener in listeners {
            assert!(bundled(listener), "{listener}");
        }
    }
    for clip in &plan.clips {
        assert!(plan.stt.contains_key(&clip.language), "{}", clip.url);
        assert_eq!(clip.sha256.len(), 64, "{}", clip.url);
    }
    assert!(!plan.vad.builds.is_empty());
    for detector in &plan.vad.builds {
        assert!(bundled(detector), "{detector}");
    }
    assert!(!plan.end_of_turn.builds.is_empty());
    for model in &plan.end_of_turn.builds {
        assert!(bundled(&model.build), "{}", model.build);
        for language in &model.required {
            assert!(
                plan.stt.contains_key(language),
                "{language}: no clip hears it"
            );
        }
    }
    assert!(
        plan.end_of_turn
            .builds
            .iter()
            .any(|model| !model.required.is_empty()),
        "something is required"
    );
    assert_eq!(primary("es-ES"), "es");
}

#[test]
fn a_plan_with_a_language_and_no_sentence_for_it_is_refused() {
    let json = r#"{"max_wer": 0.2, "sentences": {}, "tts": [{"build": "m/b", "voices": {"es": null}}],
        "stt": {}, "clips": [], "vad": {"builds": [], "silence_s": 1, "tolerance_s": 0.3, "min_coverage": 0.5},
        "end_of_turn": {"builds": [], "pause_s": 0.2, "pause_floor": 0.01, "threshold": 0.5}}"#;
    assert!(plan(json).is_err());
    assert!(plan(r#"{"max_wer": 0.2}"#).is_err(), "strict");
}

#[test]
fn audio_is_resampled_linearly_for_a_stream() {
    assert_eq!(
        resample(&[0.0, 1.0, 0.0, -1.0], 8, 8),
        [0.0, 1.0, 0.0, -1.0]
    );
    assert_eq!(resample(&[0.0, 1.0, 0.0, -1.0], 8, 4), [0.0, 0.0]);
    assert_eq!(resample(&[0.0, 1.0], 4, 8), [0.0, 0.5, 1.0, 1.0]);
}

#[test]
fn a_detection_is_the_clips_speech_between_its_silences() {
    let plan = VadPlan {
        builds: Vec::new(),
        silence_s: 1.0,
        tolerance_s: 0.3,
        min_coverage: 0.5,
    };
    let (samples, clip) = padded(&[0.5; 3], 2, 1.0);
    assert_eq!(samples, [0.0, 0.0, 0.5, 0.5, 0.5, 0.0, 0.0]);
    assert_eq!(clip, 1.0..2.5);

    let clip = 1.0..5.0;
    assert_eq!(judge(&[1.1..3.0, 3.5..4.9], false, &clip, &plan), Ok(()));
    assert_eq!(
        judge(&[0.8..4.0], false, &clip, &plan),
        Ok(()),
        "within the tolerance"
    );
    let why = |segments: &[std::ops::Range<f64>], finished| {
        judge(segments, finished, &clip, &plan).unwrap_err()
    };
    assert_eq!(why(&[], false), "no speech found");
    assert!(
        why(&[0.2..4.0], false).contains("outside the clip"),
        "speech in the silence before"
    );
    assert!(
        why(&[1.0..5.6], false).contains("outside the clip"),
        "in the silence after"
    );
    assert!(why(&[1.0..2.0], false).contains("covers 25%"));
    assert_eq!(
        why(&[1.0..5.0], true),
        "the speech had not ended when the audio did"
    );
}

#[test]
fn a_turn_is_heard_whole_and_cut_each_with_its_pause_and_judged_by_both() {
    let plan = EndOfTurnPlan {
        builds: Vec::new(),
        pause_s: 0.5,
        pause_floor: 0.01,
        threshold: 0.5,
    };
    // At 10 Hz, windows of one sample: speech in 1..3 and in 4..8, the longer, cut in its middle.
    let clip = [0.0, 0.5, 0.5, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0];
    assert_eq!(end_of_turn::cut_point(&clip, 10, plan.pause_floor), 6);
    let (whole, cut) = end_of_turn::heard(&clip, 10, &plan);
    assert_eq!(whole.len(), 10 + 5);
    assert_eq!(cut.len(), 6 + 5);
    assert!(cut[6..].iter().all(|s| *s == 0.0));
    assert_eq!(end_of_turn::judge(0.9, 0.1, &plan), Ok(()));
    assert!(
        end_of_turn::judge(0.4, 0.1, &plan).is_err(),
        "whole not complete"
    );
    assert!(end_of_turn::judge(0.9, 0.5, &plan).is_err(), "cut complete");
}
