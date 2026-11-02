//! Which of the engines linked into the engine this OS can build: the backends whose library does not compile
//! everywhere, and what they are built with. Only a value differs, so there is no trait: each backend has its
//! condition, written once here as a macro that gates the items it is given, and as a constant that says the same, for
//! the tests that check which backends a build holds.

/// Whether the whisper.cpp backend is built: not on Android, where speech to text is sherpa-onnx's and `whisper-rs`,
/// which compiles whisper.cpp with CMake and a C++ compiler, is not a dependency (Cargo.toml). The macro below is the
/// same condition.
#[cfg(test)]
pub const WHISPER_CPP: bool = cfg!(not(target_os = "android"));

/// Whether the whisper.cpp backend is built with ggml's Metal backend: on macOS on Apple silicon and on iOS, where
/// Cargo.toml turns on whisper-rs's `metal` feature (the same condition). Elsewhere it runs on the CPU only.
#[cfg_attr(
    any(web, target_os = "android"),
    allow(
        dead_code,
        reason = "whisper.cpp is built natively only, and not on Android"
    )
)]
pub const WHISPER_CPP_METAL: bool = cfg!(any(
    all(target_os = "macos", target_arch = "aarch64"),
    target_os = "ios"
));

/// The items it is given, where [`WHISPER_CPP`] is true.
macro_rules! where_whisper_cpp_builds {
    ($($item:item)*) => {
        $(
            #[cfg(not(target_os = "android"))]
            $item
        )*
    };
}
pub(crate) use where_whisper_cpp_builds;
