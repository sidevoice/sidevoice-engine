//! The part of sherpa-onnx's C API (`sherpa-onnx/c-api/c-api.h`, v1.13.8) this backend calls, declared by hand.
//!
//! The configs are plain C structs that the library reads whole, so each one here has the header's exact layout: the
//! fields this backend sets are named, and every run of fields it leaves zeroed (whole model families it does not
//! load) is a [`Reserved`] block of the same size. The C API treats zero as "default" for every field, so a zeroed
//! config plus the named fields is what the header's own examples build. The tests check every size and named offset
//! against the header's, measured with a C compiler (`tests.rs`); bumping the library's version means checking the
//! header again, since a new field in the middle of a struct moves everything after it.
//!
//! Every pointer is 8 bytes and every `int32_t` and `float` 4 on each platform `backends.json` lists for sherpa-onnx
//! (64-bit macOS, Linux and Windows), so one layout serves them all.

use std::ffi::c_char;

/// Fields left zeroed, `N` pointer-sized words: same size and alignment as the C fields they stand for.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Reserved<const N: usize>([usize; N]);

impl<const N: usize> Default for Reserved<N> {
    fn default() -> Self {
        Self([0; N])
    }
}

/// `SherpaOnnxFeatureConfig`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct FeatureConfig {
    pub(super) sample_rate: i32,
    pub(super) feature_dim: i32,
}

/// `SherpaOnnxOfflineTransducerModelConfig`, whole: the transducers NeMo exports (FastConformer, Parakeet) among them.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct TransducerModelConfig {
    pub(super) encoder: *const c_char,
    pub(super) decoder: *const c_char,
    pub(super) joiner: *const c_char,
}

/// `SherpaOnnxOfflineWhisperModelConfig`, whole.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct WhisperModelConfig {
    pub(super) encoder: *const c_char,
    pub(super) decoder: *const c_char,
    pub(super) language: *const c_char,
    pub(super) task: *const c_char,
    pub(super) tail_paddings: i32,
    pub(super) enable_token_timestamps: i32,
    pub(super) enable_segment_timestamps: i32,
}

/// `SherpaOnnxOfflineModelConfig`: the transducer's and Whisper's fields, and the common ones.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct OfflineModelConfig {
    pub(super) transducer: TransducerModelConfig,
    /// `paraformer`, `nemo_ctc`.
    pub(super) before_whisper: Reserved<2>,
    pub(super) whisper: WhisperModelConfig,
    /// `tdnn`.
    pub(super) tdnn: Reserved<1>,
    pub(super) tokens: *const c_char,
    pub(super) num_threads: i32,
    pub(super) debug: i32,
    pub(super) provider: *const c_char,
    /// `model_type`, `modeling_unit`, `bpe_vocab`, `telespeech_ctc`, and every model family after them, from
    /// `sense_voice` to `cohere_transcribe`.
    pub(super) after_provider: Reserved<48>,
}

/// `SherpaOnnxOfflineRecognizerConfig`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct OfflineRecognizerConfig {
    pub(super) feat_config: FeatureConfig,
    pub(super) model_config: OfflineModelConfig,
    /// `lm_config`.
    pub(super) lm_config: Reserved<2>,
    pub(super) decoding_method: *const c_char,
    /// `max_active_paths`, `hotwords_file`, `hotwords_score`, `rule_fsts`, `rule_fars`, `blank_penalty`, `hr`.
    pub(super) after_decoding_method: Reserved<9>,
}

/// `SherpaOnnxOfflineRecognizerResult`: its text, and the rest left unread.
#[repr(C)]
pub(super) struct OfflineRecognizerResult {
    pub(super) text: *const c_char,
    /// `timestamps` to `segment_count`.
    pub(super) rest: Reserved<15>,
}

/// `SherpaOnnxOfflineTtsVitsModelConfig`, whole: Piper's voices are VITS models.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct VitsModelConfig {
    pub(super) model: *const c_char,
    pub(super) lexicon: *const c_char,
    pub(super) tokens: *const c_char,
    pub(super) data_dir: *const c_char,
    pub(super) noise_scale: f32,
    pub(super) noise_scale_w: f32,
    pub(super) length_scale: f32,
    pub(super) dict_dir: *const c_char,
}

/// `SherpaOnnxOfflineTtsKokoroModelConfig`, whole.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct KokoroModelConfig {
    pub(super) model: *const c_char,
    pub(super) voices: *const c_char,
    pub(super) tokens: *const c_char,
    pub(super) data_dir: *const c_char,
    pub(super) length_scale: f32,
    pub(super) dict_dir: *const c_char,
    pub(super) lexicon: *const c_char,
    pub(super) lang: *const c_char,
}

/// `SherpaOnnxOfflineTtsSupertonicModelConfig`, whole.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct SupertonicModelConfig {
    pub(super) duration_predictor: *const c_char,
    pub(super) text_encoder: *const c_char,
    pub(super) vector_estimator: *const c_char,
    pub(super) vocoder: *const c_char,
    pub(super) tts_json: *const c_char,
    pub(super) unicode_indexer: *const c_char,
    pub(super) voice_style: *const c_char,
}

/// `SherpaOnnxOfflineTtsModelConfig`: the VITS, Kokoro and Supertonic fields, and the common ones.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct OfflineTtsModelConfig {
    pub(super) vits: VitsModelConfig,
    pub(super) num_threads: i32,
    pub(super) debug: i32,
    pub(super) provider: *const c_char,
    /// `matcha`.
    pub(super) matcha: Reserved<7>,
    pub(super) kokoro: KokoroModelConfig,
    /// `kitten`, `zipvoice`, `pocket`.
    pub(super) after_kokoro: Reserved<21>,
    pub(super) supertonic: SupertonicModelConfig,
}

/// `SherpaOnnxOfflineTtsConfig`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct OfflineTtsConfig {
    pub(super) model: OfflineTtsModelConfig,
    /// `rule_fsts`, `max_num_sentences`, `rule_fars`, `silence_scale`.
    pub(super) after_model: Reserved<4>,
}

/// `SherpaOnnxGenerationConfig`, whole.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct GenerationConfig {
    pub(super) silence_scale: f32,
    pub(super) speed: f32,
    pub(super) sid: i32,
    pub(super) reference_audio: *const f32,
    pub(super) reference_audio_len: i32,
    pub(super) reference_sample_rate: i32,
    pub(super) reference_text: *const c_char,
    pub(super) num_steps: i32,
    pub(super) extra: *const c_char,
}

/// `SherpaOnnxGeneratedAudio`, whole.
#[repr(C)]
pub(super) struct GeneratedAudio {
    pub(super) samples: *const f32,
    pub(super) n: i32,
    pub(super) sample_rate: i32,
}

/// `SherpaOnnxOfflineRecognizer`, `SherpaOnnxOfflineStream` and `SherpaOnnxOfflineTts`: opaque handles.
#[repr(C)]
pub(super) struct OfflineRecognizer {
    _opaque: [u8; 0],
}

#[repr(C)]
pub(super) struct OfflineStream {
    _opaque: [u8; 0],
}

#[repr(C)]
pub(super) struct OfflineTts {
    _opaque: [u8; 0],
}

/// The C API's own advice: zero the whole config, then set what is needed. Every field here is a null pointer, an
/// integer or a float, for all of which zero is a valid value and the C API's "default".
macro_rules! zeroed_default {
    ($($config:ty),*) => {$(
        impl Default for $config {
            fn default() -> Self {
                // SAFETY: a `repr(C)` struct of raw pointers, integers, floats and nested such structs: all zeroes is
                // a valid value of each.
                unsafe { std::mem::zeroed() }
            }
        }
    )*};
}

zeroed_default!(OfflineRecognizerConfig, OfflineTtsConfig, GenerationConfig);
