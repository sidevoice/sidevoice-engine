//! Text to the phonemes Kokoro reads, through eSpeak NG compiled to WebAssembly (the npm package `espeak-ng`, with
//! its full language data; kokoro-js's own phonemizer only speaks English). The approach is sidevoice-web's
//! `packages/browser-audio`: eSpeak NG writes IPA for each stretch of text between punctuation marks, the marks are
//! kept as they are, and the IPA is brought to Misaki's conventions (its `EspeakG2P`), which Kokoro was trained on.
//!
//! eSpeak NG is under the GPL-3.0-or-later, unlike the engine (sidevoice-engine#30).

use js_sys::Reflect;
use wasm_bindgen::prelude::*;

use super::{call, object};

#[cfg(test)]
mod tests;

#[wasm_bindgen(inline_js = "export function importEspeak() { return import('espeak-ng'); }")]
extern "C" {
    /// The npm package's `espeak-ng`, imported when first asked for.
    #[wasm_bindgen(catch, js_name = importEspeak)]
    async fn import_espeak() -> Result<JsValue, JsValue>;
}

/// eSpeak NG, imported: its factory, which runs it once per call as its command line would.
pub(super) struct Espeak {
    factory: JsValue,
}

impl Espeak {
    pub(super) async fn import() -> Result<Self, JsValue> {
        let module = import_espeak().await?;
        Ok(Self {
            factory: Reflect::get(&module, &"default".into())?,
        })
    }

    /// `text` in the eSpeak NG voice `voice` ("es", "en-us", ...), as Kokoro's phonemes.
    pub(super) async fn phonemes(&self, text: &str, voice: &str) -> Result<String, JsValue> {
        let mut ipa = String::new();
        for (part, punctuation) in split(text) {
            if punctuation || part.trim().is_empty() {
                ipa.push_str(part);
            } else {
                ipa.push_str(self.ipa(part, voice).await?.trim());
            }
        }
        Ok(misaki(&ipa, voice.starts_with("en")))
    }

    /// What eSpeak NG writes for `text` with `--ipa=3`: IPA, with ties between the phonemes of one sound.
    async fn ipa(&self, text: &str, voice: &str) -> Result<String, JsValue> {
        let arguments: js_sys::Array = [
            "--phonout",
            "/phonemes",
            "-q",
            "-b",
            "1",
            "--ipa=3",
            "-v",
            voice,
            text,
        ]
        .into_iter()
        .map(JsValue::from)
        .collect();
        let quiet = js_sys::Function::new_no_args("");
        let options = object(&[
            ("arguments", arguments.into()),
            ("print", quiet.clone().into()),
            ("printErr", quiet.into()),
        ]);
        let instance = call(&self.factory, &JsValue::UNDEFINED, &[options]).await?;
        let fs = Reflect::get(&instance, &"FS".into())?;
        let read = Reflect::get(&fs, &"readFile".into())?;
        let encoding = object(&[("encoding", "utf8".into())]);
        let phonemes = call(&read, &fs, &["/phonemes".into(), encoding]).await?;
        Ok(phonemes.as_string().unwrap_or_default())
    }
}

/// The punctuation eSpeak NG drops, which Kokoro reads as it is (browser-audio's set).
fn is_punctuation(c: char) -> bool {
    ";:,.!?¡¿—…“”\"()".contains(c)
}

/// `text` in runs of punctuation and of everything else, in order, each said to be punctuation or not.
fn split(text: &str) -> Vec<(&str, bool)> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut current = None;
    for (at, c) in text.char_indices() {
        let punctuation = is_punctuation(c);
        if current.is_some_and(|kind| kind != punctuation) {
            parts.push((&text[start..at], current.unwrap_or_default()));
            start = at;
        }
        current = Some(punctuation);
    }
    if let Some(kind) = current {
        parts.push((&text[start..], kind));
    }
    parts
}

/// eSpeak NG's IPA (`--ipa=3`) as Misaki's `EspeakG2P` writes it: its language switches (`(en)`) dropped, the tied
/// sounds of other languages each written as the one letter Misaki gives it, and English's as Misaki's English.
pub(super) fn misaki(ipa: &str, english: bool) -> String {
    let mut text = without_switches(&ipa.replace(['\u{200d}', '\u{361}'], "^"));
    if english {
        text = text
            .replace('^', "")
            .replace('ʲ', "j")
            .replace('r', "ɹ")
            .replace('x', "k")
            .replace('ɬ', "l");
    } else {
        const TIED: &[(&str, &str)] = &[
            ("a^ɪ", "I"),
            ("a^ʊ", "W"),
            ("d^z", "ʣ"),
            ("d^ʒ", "ʤ"),
            ("e^ɪ", "A"),
            ("o^ʊ", "O"),
            ("ə^ʊ", "Q"),
            ("s^s", "S"),
            ("t^s", "ʦ"),
            ("t^ʃ", "ʧ"),
            ("ɔ^ɪ", "Y"),
        ];
        for (from, to) in TIED {
            text = text.replace(from, to);
        }
        text = text.replace(['^', '-'], "");
    }
    text.trim().to_owned()
}

/// `text` without eSpeak NG's switches to another language's voice: `(en)`, `(es)`, `(en-us)`, ...
fn without_switches(text: &str) -> String {
    const LANGUAGES: &[&str] = &["en", "es", "fr", "it", "pt", "hi"];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let switch = after.find(')').filter(|&close| {
            let inside = &after[..close];
            let (language, region) = match inside.split_once('-') {
                Some((language, region)) => (language, Some(region)),
                None => (inside, None),
            };
            let region_ok = region.is_none_or(|region| {
                !region.is_empty() && region.chars().all(|c| c.is_ascii_lowercase())
            });
            LANGUAGES.contains(&language) && region_ok
        });
        match switch {
            Some(close) => rest = &after[close + 1..],
            None => {
                out.push('(');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
