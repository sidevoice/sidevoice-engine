//! Opening the sherpa-onnx library at run time: the C API's shared library and ONNX Runtime beside it, from the
//! directory the installer unpacked the `library` archive of `backends.json` into, with the functions this backend
//! calls resolved once into [`Api`].

use std::env::consts::{DLL_PREFIX, DLL_SUFFIX};
use std::ffi::{c_char, c_void};
use std::fs;
use std::path::{Path, PathBuf};

use libloading::Library;

use super::c_api::{
    GeneratedAudio, GenerationConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineRecognizerResult, OfflineStream, OfflineTts, OfflineTtsConfig,
};
use crate::{Error, Result};

/// The C API's library and ONNX Runtime, as every platform's archive names them (all under `lib/`), without the
/// platform's prefix and suffix (`libsherpa-onnx-c-api.so`, `libsherpa-onnx-c-api.dylib`, `sherpa-onnx-c-api.dll`).
const C_API: &str = "sherpa-onnx-c-api";
const RUNTIME: &str = "onnxruntime";

/// How deep below the installed path the C API's library may be: the archives put it at `<root>/lib/`.
const MAX_DEPTH: usize = 3;

/// `SherpaOnnxGeneratedAudioProgressCallbackWithArg`: this backend passes none.
type ProgressCallback = unsafe extern "C" fn(*const f32, i32, f32, *mut c_void) -> i32;

/// The library's functions this backend calls, and the open libraries they live in, which stay open as long as this
/// does: every loaded model holds one through an `Arc`, so the library follows its models.
pub(super) struct Api {
    pub(super) create_offline_recognizer:
        unsafe extern "C" fn(*const OfflineRecognizerConfig) -> *const OfflineRecognizer,
    pub(super) offline_recognizer_set_config:
        unsafe extern "C" fn(*const OfflineRecognizer, *const OfflineRecognizerConfig),
    pub(super) destroy_offline_recognizer: unsafe extern "C" fn(*const OfflineRecognizer),
    pub(super) create_offline_stream:
        unsafe extern "C" fn(*const OfflineRecognizer) -> *const OfflineStream,
    pub(super) destroy_offline_stream: unsafe extern "C" fn(*const OfflineStream),
    pub(super) accept_waveform_offline:
        unsafe extern "C" fn(*const OfflineStream, i32, *const f32, i32),
    pub(super) decode_offline_stream:
        unsafe extern "C" fn(*const OfflineRecognizer, *const OfflineStream),
    pub(super) get_offline_stream_result:
        unsafe extern "C" fn(*const OfflineStream) -> *const OfflineRecognizerResult,
    pub(super) destroy_offline_recognizer_result:
        unsafe extern "C" fn(*const OfflineRecognizerResult),
    pub(super) create_offline_tts:
        unsafe extern "C" fn(*const OfflineTtsConfig) -> *const OfflineTts,
    pub(super) destroy_offline_tts: unsafe extern "C" fn(*const OfflineTts),
    pub(super) offline_tts_sample_rate: unsafe extern "C" fn(*const OfflineTts) -> i32,
    pub(super) offline_tts_num_speakers: unsafe extern "C" fn(*const OfflineTts) -> i32,
    pub(super) offline_tts_generate_with_config: unsafe extern "C" fn(
        *const OfflineTts,
        *const c_char,
        *const GenerationConfig,
        Option<ProgressCallback>,
        *mut c_void,
    ) -> *const GeneratedAudio,
    pub(super) destroy_offline_tts_generated_audio: unsafe extern "C" fn(*const GeneratedAudio),
    // Dropped in this order: the C API before the runtime it needs.
    _c_api: Library,
    _runtime: Option<Library>,
}

impl Api {
    /// Opens the library installed at `installed`: the directory the `library` archive was unpacked into (the C API's
    /// library is found below it), or the C API's library itself. ONNX Runtime, beside it, is opened first, so that
    /// the C API finds it already loaded whatever the platform's search rules. Fails with `library-open-failed`.
    pub(super) fn open(installed: &Path) -> Result<Self> {
        let failed = Error::new("library-open-failed");
        let c_api = if installed.is_file() {
            installed.to_path_buf()
        } else {
            find(installed, &file_name(C_API), MAX_DEPTH).ok_or(failed)?
        };
        let runtime = c_api.with_file_name(file_name(RUNTIME));
        // SAFETY: loading a library runs its initialisers. These are sherpa-onnx's and ONNX Runtime's, from the
        // archive pinned by digest in backends.json, whose initialisers have no preconditions.
        let runtime = if runtime.is_file() {
            Some(unsafe { Library::new(&runtime) }.map_err(|_| failed)?)
        } else {
            None
        };
        // SAFETY: as above.
        let lib = unsafe { Library::new(&c_api) }.map_err(|_| failed)?;

        macro_rules! resolve {
            ($name:literal) => {
                // SAFETY: each field's type is the function's signature in c-api.h (v1.13.8, the version
                // backends.json pins), and the pointer is used only while `lib`, kept in `Api`, is open.
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }.map_err(|_| failed)?
            };
        }

        Ok(Self {
            create_offline_recognizer: resolve!("SherpaOnnxCreateOfflineRecognizer"),
            offline_recognizer_set_config: resolve!("SherpaOnnxOfflineRecognizerSetConfig"),
            destroy_offline_recognizer: resolve!("SherpaOnnxDestroyOfflineRecognizer"),
            create_offline_stream: resolve!("SherpaOnnxCreateOfflineStream"),
            destroy_offline_stream: resolve!("SherpaOnnxDestroyOfflineStream"),
            accept_waveform_offline: resolve!("SherpaOnnxAcceptWaveformOffline"),
            decode_offline_stream: resolve!("SherpaOnnxDecodeOfflineStream"),
            get_offline_stream_result: resolve!("SherpaOnnxGetOfflineStreamResult"),
            destroy_offline_recognizer_result: resolve!("SherpaOnnxDestroyOfflineRecognizerResult"),
            create_offline_tts: resolve!("SherpaOnnxCreateOfflineTts"),
            destroy_offline_tts: resolve!("SherpaOnnxDestroyOfflineTts"),
            offline_tts_sample_rate: resolve!("SherpaOnnxOfflineTtsSampleRate"),
            offline_tts_num_speakers: resolve!("SherpaOnnxOfflineTtsNumSpeakers"),
            offline_tts_generate_with_config: resolve!("SherpaOnnxOfflineTtsGenerateWithConfig"),
            destroy_offline_tts_generated_audio: resolve!(
                "SherpaOnnxDestroyOfflineTtsGeneratedAudio"
            ),
            _c_api: lib,
            _runtime: runtime,
        })
    }
}

/// The file name of the shared library `name` on this platform.
fn file_name(name: &str) -> String {
    format!("{DLL_PREFIX}{name}{DLL_SUFFIX}")
}

/// The file called `name` in `dir` or below it, at most `depth` directories down, shallowest first.
fn find(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let here = dir.join(name);
    if here.is_file() {
        return Some(here);
    }
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    subdirs.sort();
    subdirs
        .iter()
        .find_map(|subdir| find(subdir, name, depth - 1))
}
