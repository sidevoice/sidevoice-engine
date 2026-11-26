//! whisper.cpp on a machine without the Vulkan loader: a small Whisper model loads and transcribes on the CPU, and the
//! engine says so. The engine does not link the loader (src/platform/vulkan.rs), so nothing here may crash for the
//! lack of it. It does something only when `SIDEVOICE_NO_VULKAN_LOADER` is set, by the CI step that removes `libvulkan1` first, then runs this ignored
//! test (`cargo test --test whisper_no_loader -- --ignored`).

#![cfg(native)]

use std::future::Future;

use sidevoice_engine::{Accelerator, BundledCatalog, Cancel, Engine, NativeHost, Progress};

const BUILD: &str = "whisper-tiny/whisper-cpp-q5_1";

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a Tokio runtime")
        .block_on(future)
}

#[test]
#[ignore = "downloads a Whisper model, and means something only without the Vulkan loader: the e2e workflow runs it"]
fn a_whisper_model_loads_on_the_cpu_without_the_vulkan_loader() {
    // Only where the loader is known to be missing: `cargo test -- --include-ignored` runs this everywhere else too, on
    // machines (a Mac, which runs on Metal) where its assertions do not apply.
    if std::env::var_os("SIDEVOICE_NO_VULKAN_LOADER").is_none() {
        return;
    }
    let dir = std::env::temp_dir().join("sidevoice-whisper-no-loader");
    let host = NativeHost::new(dir).expect("a host");
    let engine = Engine::new(Box::new(host), vec![Box::new(BundledCatalog)]).expect("an engine");
    let models = block_on(engine.local_catalog().models(None)).expect("the models");
    let (model, entry) = models
        .iter()
        .find_map(|model| {
            let entry = model.builds.iter().find(|entry| entry.id == BUILD)?;
            Some((model, entry))
        })
        .expect("the build is in the catalogue");
    assert!(entry.available, "{BUILD}: {:?}", entry.reasons);
    assert_eq!(
        entry.accelerator,
        Some(Accelerator::Cpu),
        "without the loader the build is offered on the CPU"
    );
    let loaded = block_on(engine.load(&model.id, Some(BUILD), &|_: Progress| {}, &Cancel::new()))
        .expect("the model loads");
    assert_eq!(loaded.accelerator().actual, Some(Accelerator::Cpu));
    // A second of silence: it runs the model, and says whatever it says.
    let silence = vec![0.0_f32; 16_000];
    let stt = loaded.as_stt().expect("speech to text");
    block_on(stt.transcribe(&silence, 16_000, Some("en"))).expect("it transcribes");
}
