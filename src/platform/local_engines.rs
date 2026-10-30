//! Which of the engines linked into the engine this OS can build: the backends whose library does not compile
//! everywhere. Only a value differs, so there is no trait: each backend has its condition, written once here as a macro
//! that gates the items it is given, and as a constant that says the same, for the tests that check which backends a
//! build holds.

/// Whether the whisper.cpp backend is built: not on a phone (Android, iOS), where speech to text is sherpa-onnx's and
/// `whisper-rs`, which compiles whisper.cpp with CMake and a C++ compiler, is not a dependency (Cargo.toml). The macro
/// below is the same condition.
#[cfg(test)]
pub const WHISPER_CPP: bool = cfg!(not(any(target_os = "android", target_os = "ios")));

/// The items it is given, where [`WHISPER_CPP`] is true.
macro_rules! where_whisper_cpp_builds {
    ($($item:item)*) => {
        $(
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            $item
        )*
    };
}
pub(crate) use where_whisper_cpp_builds;

/// Whether the ONNX Runtime backend is built: not on iOS, whose sherpa-onnx xcframework keeps its ONNX Runtime private
/// and exports no `OrtGetApiBase` for `ort` to resolve (Cargo.toml). The macro below is the same condition.
#[cfg(test)]
pub const ONNX_RUNTIME: bool = cfg!(not(target_os = "ios"));

/// The items it is given, where [`ONNX_RUNTIME`] is true.
macro_rules! where_onnx_runtime_builds {
    ($($item:item)*) => {
        $(
            #[cfg(not(target_os = "ios"))]
            $item
        )*
    };
}
pub(crate) use where_onnx_runtime_builds;
