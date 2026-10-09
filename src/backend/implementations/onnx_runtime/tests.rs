//! What the ONNX Runtime backend does before a model runs: it finds the runtime sherpa-onnx links (opening is
//! `ort` taking its C API, which proves the link and the API version), and refuses what it cannot load. Running
//! smart-turn for real is the voice loop's (`tests/voice_loop.rs`).

use super::{OnnxRuntime, SPEC};
use crate::backend::{Backend, BackendModel};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::test_support::{build, ready};
use crate::Result;

fn installed(files: &[(&str, &str)]) -> Installed {
    Installed {
        files: files
            .iter()
            .map(|(key, path)| ((*key).to_owned(), (*path).to_owned()))
            .collect(),
    }
}

fn load(accelerator: Accelerator, files: &[(&str, &str)]) -> Result<Box<dyn BackendModel>> {
    let files = installed(files);
    let library = ready(OnnxRuntime.open(&files))?;
    ready(library.load(&build("test", "onnxruntime", 0), accelerator, &files))
}

fn code(result: Result<Box<dyn BackendModel>>) -> &'static str {
    result.map(drop).unwrap_err().code
}

#[test]
fn it_runs_on_the_cpu_and_opens_on_the_runtime_sherpa_onnx_links() {
    assert_eq!(SPEC.accelerators, [Accelerator::Cpu]);
    assert!(ready(OnnxRuntime.open(&Installed::default())).is_ok());
    assert!(
        ready(OnnxRuntime.open(&Installed::default())).is_ok(),
        "twice"
    );
}

#[test]
fn what_it_cannot_load_says_why() {
    let missing = [("smart_turn", "/nonexistent/smart-turn.onnx")];
    assert_eq!(code(load(Accelerator::Cpu, &missing)), "model-load-failed");
    assert_eq!(
        code(load(Accelerator::Metal, &missing)),
        "unsupported-accelerator"
    );
    assert_eq!(code(load(Accelerator::Cpu, &[])), "file-not-installed");
    let other = [("whisper.encoder", "e.onnx")];
    assert_eq!(code(load(Accelerator::Cpu, &other)), "unsupported-model");
}
