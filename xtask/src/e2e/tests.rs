//! The loop's arithmetic and its plan, without running a model: word error rates, WAV files, resampling, and that the
//! plan names models the bundled catalogue has on sherpa-onnx.

use sidevoice_engine::{BundledCatalog, CatalogSource, ModelEntry};

use super::audio::{read_wav, wav};
use super::{plan, primary, BACKEND};
use crate::wer::{normalised, wer};
use crate::{read, repo};

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
fn the_plan_names_bundled_models_on_sherpa_onnx_and_has_a_sentence_for_each_language() {
    let plan = plan(&String::from_utf8(read(&repo().join("xtask/e2e.json")).unwrap()).unwrap())
        .expect("e2e.json");
    let catalogue = BundledCatalog.load().expect("the bundled catalogue");
    let models: Vec<&ModelEntry> = catalogue
        .families
        .iter()
        .flat_map(|family| &family.models)
        .collect();
    let on_sherpa = |id: &str| {
        models
            .iter()
            .find(|model| model.id == id)
            .is_some_and(|model| model.builds.iter().any(|build| build.backend == BACKEND))
    };
    for speaker in &plan.tts {
        assert!(on_sherpa(&speaker.model), "{}", speaker.model);
        assert!(!speaker.voices.is_empty(), "{}", speaker.model);
    }
    for (language, listeners) in &plan.stt {
        assert!(plan.sentences.contains_key(language), "{language}");
        for listener in listeners {
            assert!(on_sherpa(listener), "{listener}");
        }
    }
    for clip in &plan.clips {
        assert!(plan.stt.contains_key(&clip.language), "{}", clip.url);
        assert_eq!(clip.sha256.len(), 64, "{}", clip.url);
    }
    assert_eq!(primary("es-ES"), "es");
}

#[test]
fn a_plan_with_a_language_and_no_sentence_for_it_is_refused() {
    let json = r#"{"max_wer": 0.2, "sentences": {}, "tts": [{"model": "m", "voices": {"es": null}}],
        "stt": {}, "clips": []}"#;
    assert!(plan(json).is_err());
    assert!(plan(r#"{"max_wer": 0.2}"#).is_err(), "strict");
}

#[test]
fn a_voice_with_its_own_limit_must_say_why() {
    let json = |extra: &str| {
        format!(
            r#"{{"max_wer": 0.2, "sentences": {{"es": "hola"}}, "stt": {{}}, "clips": [],
            "tts": [{{"model": "m", "voices": {{"es": null}}{extra}}}]}}"#
        )
    };
    assert!(plan(&json("")).is_ok());
    assert!(plan(&json(r#", "max_wer": 0.4"#)).is_err());
    assert!(plan(&json(r#", "max_wer": 0.4, "why": "a weak voice""#)).is_ok());
}
