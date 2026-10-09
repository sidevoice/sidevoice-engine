use super::{misaki, split, without_switches};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn text_splits_into_runs_of_punctuation_and_of_words() {
    assert_eq!(
        split("¡Hola, mundo!"),
        [
            ("¡", true),
            ("Hola", false),
            (",", true),
            (" mundo", false),
            ("!", true)
        ]
    );
    assert_eq!(split("sin puntuación"), [("sin puntuación", false)]);
    assert_eq!(split("..."), [("...", true)]);
    assert!(split("").is_empty());
}

#[wasm_bindgen_test]
fn language_switches_are_dropped_and_other_parentheses_kept() {
    assert_eq!(without_switches("(en)hˈəʊ(es)la"), "hˈəʊla");
    assert_eq!(without_switches("a(en-us)b"), "ab");
    assert_eq!(without_switches("a(de)b (x) (en-)c"), "a(de)b (x) (en-)c");
}

#[wasm_bindgen_test]
fn tied_sounds_become_misakis_letters_outside_english() {
    // eSpeak NG ties the sounds of one phoneme with U+0361 or U+200D.
    assert_eq!(misaki("t\u{361}ʃˈika", false), "ʧˈika");
    assert_eq!(misaki("ˈa\u{200d}ɪɾe", false), "ˈIɾe");
    assert_eq!(misaki("(en)ˈo\u{361}ʊ-ke", false), "ˈOke");
}

#[wasm_bindgen_test]
fn english_keeps_its_ties_as_two_letters_and_takes_misakis_r() {
    assert_eq!(misaki("ɹˈe\u{361}ɪn", true), "ɹˈeɪn");
    assert_eq!(misaki("rˈɛd", true), "ɹˈɛd");
}
