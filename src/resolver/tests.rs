//! The funnel's accelerator and requirement steps, on hosts that report little: what the host cannot tell passes, what
//! it does not report is absent, a probe only narrows what it reports, and a build only runs on what it requires.

use async_trait::async_trait;

use super::Rejection;
use super::Resolver;
use crate::backend::{Backend, BackendSpec, Library, MinCores};
use crate::catalog::{BuildEntry, CatalogFragment, CatalogSource, MergedCatalog};
use crate::install::Installed;
use crate::test_support::{build, family, model};
use crate::{Accelerator, Capabilities, Capability, Error, Reason, Result, Runs};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// A backend that runs on CoreML or the CPU, needs 4 cores, and whose probe confirms `probe`. It borrows the id
/// "sherpa-onnx", a known one.
struct FixedProbeBackend {
    spec: BackendSpec,
    probe: &'static [Accelerator],
}

impl FixedProbeBackend {
    fn probing(probe: &'static [Accelerator]) -> Box<dyn Backend> {
        Box::new(Self {
            spec: BackendSpec {
                id: "sherpa-onnx",
                name: "a backend that probes as told",
                description: "A test double.",
                upstream: "https://example.com",
                accelerators: &[Accelerator::CoreMl, Accelerator::Cpu],
                requirements: &[&MinCores(4)],
            },
            probe,
        })
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for FixedProbeBackend {
    fn spec(&self) -> &BackendSpec {
        &self.spec
    }

    fn probe(&self, _caps: &Capabilities) -> Vec<Accelerator> {
        self.probe.to_vec()
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        Err(Error::new("not-implemented"))
    }
}

/// A catalogue of one model with one build for that backend that needs 2 GB and requires these accelerators.
struct OneBuildCatalog(&'static [Accelerator]);

impl CatalogSource for OneBuildCatalog {
    fn load(&self) -> Result<CatalogFragment> {
        let mut build = build("model/sherpa-onnx", "sherpa-onnx", 2_048);
        build.requires.accelerators = self.0.to_vec();
        let builds = vec![build];
        Ok(CatalogFragment {
            families: vec![family(
                "family",
                vec![model("model", Capability::Stt, builds)],
            )],
        })
    }
}

fn caps(accelerators: &[Accelerator], memory_mb: Option<u32>, cores: Option<u32>) -> Capabilities {
    Capabilities {
        runs: Runs::Native,
        os: "linux".to_owned(),
        arch: "x86_64".to_owned(),
        accelerators: accelerators.to_vec(),
        memory_mb,
        cores,
    }
}

/// Every build of every model of `capability` in `catalog`, with its model's id and where it runs here or why not: what
/// the engine lists, through `Resolver::builds`.
fn ranked(
    catalog: &MergedCatalog,
    backends: &[Box<dyn Backend>],
    caps: &Capabilities,
    capability: Capability,
) -> Vec<(String, BuildEntry, Result<Accelerator, Rejection>)> {
    let resolver = Resolver::default();
    catalog
        .entries()
        .filter(|model| model.capabilities.contains(&capability))
        .flat_map(|model| {
            resolver
                .builds(model, backends, caps)
                .into_iter()
                .map(|(build, fit)| (model.id.clone(), build.clone(), fit))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The one build `catalog` has: where it runs here, or why not.
fn only(
    catalog: &MergedCatalog,
    backends: &[Box<dyn Backend>],
    caps: &Capabilities,
) -> Result<Accelerator, Rejection> {
    let builds = ranked(catalog, backends, caps, Capability::Stt);
    let [(_, _, fit)] = <[_; 1]>::try_from(builds).expect("one build");
    fit
}

/// The accelerator the one build, requiring none, is offered on, or why it is rejected.
fn fit(probe: &'static [Accelerator], caps: &Capabilities) -> Result<Accelerator, Rejection> {
    fit_requiring(&[], probe, caps)
}

/// The accelerator the one build, requiring `accelerators`, is offered on, or why it is rejected.
fn fit_requiring(
    accelerators: &'static [Accelerator],
    probe: &'static [Accelerator],
    caps: &Capabilities,
) -> Result<Accelerator, Rejection> {
    let source = OneBuildCatalog(accelerators);
    let catalog =
        MergedCatalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    only(&catalog, &[FixedProbeBackend::probing(probe)], caps)
}

const COREML_AND_CPU: &[Accelerator] = &[Accelerator::CoreMl, Accelerator::Cpu];

#[test]
fn unknown_memory_and_cores_pass_the_requirements() {
    assert_eq!(
        fit(COREML_AND_CPU, &caps(COREML_AND_CPU, None, None)),
        Ok(Accelerator::CoreMl)
    );
}

#[test]
fn known_memory_and_cores_below_the_requirements_reject_with_the_numbers() {
    assert_eq!(
        fit(COREML_AND_CPU, &caps(COREML_AND_CPU, Some(1_024), None)),
        Err(Rejection::DoesNotFit(Reason::with_numbers(
            "memory", 2_048, 1_024
        )))
    );
    assert_eq!(
        fit(COREML_AND_CPU, &caps(COREML_AND_CPU, None, Some(2))),
        Err(Rejection::DoesNotFit(Reason::with_numbers("cores", 4, 2)))
    );
}

#[test]
fn an_accelerator_the_host_does_not_report_is_absent() {
    assert_eq!(
        fit(COREML_AND_CPU, &caps(&[], None, None)),
        Err(Rejection::BackendUnavailable(Reason::new("no-accelerator")))
    );
}

#[test]
fn a_probe_narrows_what_the_host_reports_and_never_widens_it() {
    // The probe finds CoreML unusable: the CPU is next.
    assert_eq!(
        fit(&[Accelerator::Cpu], &caps(COREML_AND_CPU, None, None)),
        Ok(Accelerator::Cpu)
    );
    // The probe claims CoreML, which the host does not report: it does not count.
    assert_eq!(
        fit(COREML_AND_CPU, &caps(&[Accelerator::Cpu], None, None)),
        Ok(Accelerator::Cpu)
    );
}

#[test]
fn a_build_runs_only_on_accelerators_it_requires_in_the_backends_order() {
    let host = caps(COREML_AND_CPU, None, None);
    // Requiring both, or nothing, the backend's preference decides.
    let both = &[Accelerator::Cpu, Accelerator::CoreMl];
    assert_eq!(
        fit_requiring(both, COREML_AND_CPU, &host),
        Ok(Accelerator::CoreMl)
    );
    // Kokoro on sherpa-onnx: the CPU only.
    assert_eq!(
        fit_requiring(&[Accelerator::Cpu], COREML_AND_CPU, &host),
        Ok(Accelerator::Cpu)
    );
    let build_accelerator = Err(Rejection::DoesNotFit(Reason::new("build-accelerator")));
    assert_eq!(
        fit_requiring(&[Accelerator::Cuda], COREML_AND_CPU, &host),
        build_accelerator
    );
    // What the build requires must still have passed the probe.
    assert_eq!(
        fit_requiring(&[Accelerator::CoreMl], &[Accelerator::Cpu], &host),
        build_accelerator
    );
}

/// Kokoro on Core ML aborts the process (sherpa-onnx's library throws where its C API catches nothing): the bundled
/// catalogue's Kokoro builds require the CPU, so even a sherpa-onnx that offered Core ML runs them on the CPU.
#[test]
fn a_bundled_kokoro_build_is_never_offered_on_core_ml() {
    let source = crate::BundledCatalog;
    let catalog =
        MergedCatalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    let builds = ranked(
        &catalog,
        &[FixedProbeBackend::probing(COREML_AND_CPU)],
        &caps(COREML_AND_CPU, Some(16_384), Some(8)),
        Capability::Tts,
    );
    let kokoro: Vec<_> = builds
        .iter()
        .filter(|(model, build, _)| model.starts_with("kokoro") && build.backend == "sherpa-onnx")
        .filter_map(|(_, _, fit)| fit.as_ref().ok().copied())
        .collect();
    assert!(!kokoro.is_empty(), "a Kokoro build on sherpa-onnx");
    assert!(kokoro
        .iter()
        .all(|accelerator| *accelerator == Accelerator::Cpu));
}

/// transformers.js's fp16 builds are its WebGPU variant: the bundled catalogue's require WebGPU, so a page that only
/// has WebAssembly is never offered one, and one that has WebGPU runs them there.
#[test]
fn a_bundled_transformers_js_fp16_build_runs_on_webgpu_only() {
    let source = crate::BundledCatalog;
    let catalog =
        MergedCatalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    let fp16_accelerators = |accelerators: &[Accelerator]| -> Vec<Accelerator> {
        let backend = || -> Box<dyn Backend> {
            Box::new(PageBackend(BackendSpec {
                id: "transformers-js",
                name: "a backend of the page",
                description: "A test double.",
                upstream: "https://example.com",
                accelerators: &[Accelerator::WebGpu, Accelerator::Wasm],
                requirements: &[],
            }))
        };
        let caps = Capabilities {
            runs: Runs::Page,
            os: "web".to_owned(),
            arch: "wasm32".to_owned(),
            accelerators: accelerators.to_vec(),
            memory_mb: Some(16_384),
            cores: Some(8),
        };
        [Capability::Stt, Capability::Tts]
            .into_iter()
            .flat_map(|capability| ranked(&catalog, &[backend()], &caps, capability))
            .filter(|(_, build, _)| build.precision == "fp16")
            .filter_map(|(_, _, fit)| fit.ok())
            .collect()
    };
    assert_eq!(fp16_accelerators(&[Accelerator::Wasm]), []);
    let on_webgpu = fp16_accelerators(&[Accelerator::Wasm, Accelerator::WebGpu]);
    assert!(!on_webgpu.is_empty(), "an fp16 build on WebGPU");
    assert!(on_webgpu
        .iter()
        .all(|accelerator| *accelerator == Accelerator::WebGpu));
}

/// A backend of the page, running on WebAssembly, with no requirement of its own.
struct PageBackend(BackendSpec);

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for PageBackend {
    fn spec(&self) -> &BackendSpec {
        &self.0
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        Err(Error::new("not-implemented"))
    }
}

/// A catalogue of one transformers.js build that takes `memory_mb` and declares WebAssembly's cap.
struct WasmCatalog(u32);

impl CatalogSource for WasmCatalog {
    fn load(&self) -> Result<CatalogFragment> {
        let mut build = build("model/transformers-js-q8", "transformers-js", self.0);
        build.requires.wasm_max_mb = Some(2_048);
        Ok(CatalogFragment {
            families: vec![family(
                "family",
                vec![model("model", Capability::Stt, vec![build])],
            )],
        })
    }
}

/// The one build of [`WasmCatalog`] taking `memory_mb`, on a host that `runs` there, offered or why not.
fn fit_wasm(memory_mb: u32, runs: Runs) -> Result<Accelerator, Rejection> {
    let source = WasmCatalog(memory_mb);
    let catalog =
        MergedCatalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    let backend = PageBackend(BackendSpec {
        id: "transformers-js",
        name: "a backend of the page",
        description: "A test double.",
        upstream: "https://example.com",
        accelerators: &[Accelerator::Wasm],
        requirements: &[],
    });
    let (os, arch) = match runs {
        Runs::Page => ("web", "wasm32"),
        Runs::Native => ("linux", "x86_64"),
    };
    let caps = Capabilities {
        runs,
        os: os.to_owned(),
        arch: arch.to_owned(),
        accelerators: vec![Accelerator::Wasm],
        memory_mb: Some(16_384),
        cores: Some(8),
    };
    only(&catalog, &[Box::new(backend)], &caps)
}

#[test]
fn in_a_page_a_build_over_its_webassembly_cap_does_not_fit_whatever_the_machine_has() {
    assert_eq!(
        fit_wasm(2_260, Runs::Page),
        Err(Rejection::DoesNotFit(Reason::with_numbers(
            "wasm-memory",
            2_260,
            2_048
        )))
    );
    assert_eq!(
        fit_wasm(2_048, Runs::Page),
        Ok(Accelerator::Wasm),
        "at the cap"
    );
}

/// A backend that runs nothing yet, as the MLX stub: everything else about it fits.
struct StubBackend(Box<dyn Backend>);

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for StubBackend {
    fn spec(&self) -> &BackendSpec {
        self.0.spec()
    }

    fn unavailable(&self) -> Option<Reason> {
        Some(Reason::new("not-implemented"))
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        Err(Error::new("not-implemented"))
    }
}

/// ENG-08: a stub's builds do not run here, with its reason, before anything about the machine is asked; so the engine
/// neither recommends nor downloads one.
#[test]
fn a_stub_backends_builds_do_not_run_here_and_say_why() {
    let source = OneBuildCatalog(&[]);
    let catalog =
        MergedCatalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    let stub = StubBackend(FixedProbeBackend::probing(COREML_AND_CPU));
    let fits = caps(COREML_AND_CPU, Some(16_384), Some(8));
    assert_eq!(
        only(&catalog, &[Box::new(stub)], &fits),
        Err(Rejection::BackendUnavailable(Reason::new(
            "not-implemented"
        )))
    );
}

/// A backend `id` that runs on `accelerators`, in that order of preference, all of which its probe confirms.
fn running_on(id: &'static str, accelerators: &'static [Accelerator]) -> Box<dyn Backend> {
    Box::new(FixedProbeBackend {
        spec: BackendSpec {
            id,
            name: "a backend that runs where it is told",
            description: "A test double.",
            upstream: "https://example.com",
            accelerators,
            requirements: &[],
        },
        probe: accelerators,
    })
}

/// A catalogue of one model whose builds are, in catalogue order: `(id, backend, required accelerators)`.
struct ManyBuildsCatalog(&'static [(&'static str, &'static str, &'static [Accelerator])]);

impl CatalogSource for ManyBuildsCatalog {
    fn load(&self) -> Result<CatalogFragment> {
        let builds = self
            .0
            .iter()
            .map(|(id, backend, accelerators)| {
                let mut build = build(id, backend, 1_024);
                build.requires.accelerators = accelerators.to_vec();
                build
            })
            .collect();
        Ok(CatalogFragment {
            families: vec![family(
                "family",
                vec![model("model", Capability::Stt, builds)],
            )],
        })
    }
}

/// The builds of [`ManyBuildsCatalog`] as the resolver ranks them on `caps`: each id, with its accelerator or `None`.
fn order(
    builds: &'static [(&'static str, &'static str, &'static [Accelerator])],
    backends: &[Box<dyn Backend>],
    caps: &Capabilities,
) -> Vec<(String, Option<Accelerator>)> {
    let source = ManyBuildsCatalog(builds);
    let catalog =
        MergedCatalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    ranked(&catalog, backends, caps, Capability::Stt)
        .into_iter()
        .map(|(_, build, fit)| (build.id, fit.ok()))
        .collect()
}

/// Native builds in an order no ranking would pick: a CPU-only backend, a build its backend could run on Core ML but
/// that requires the CPU, a stub's, one that runs on Core ML, and one that runs on Metal.
const NATIVE: &[(&str, &str, &[Accelerator])] = &[
    ("model/onnxruntime", "onnxruntime", &[]),
    ("model/sherpa-onnx-cpu", "sherpa-onnx", &[Accelerator::Cpu]),
    ("model/mlx", "mlx", &[]),
    ("model/sherpa-onnx", "sherpa-onnx", &[]),
    ("model/whisper-cpp", "whisper-cpp", &[]),
];

fn native_backends() -> Vec<Box<dyn Backend>> {
    vec![
        running_on("onnxruntime", &[Accelerator::Cpu]),
        running_on("sherpa-onnx", COREML_AND_CPU),
        Box::new(StubBackend(running_on("mlx", &[Accelerator::Metal]))),
        running_on("whisper-cpp", &[Accelerator::Metal, Accelerator::Cpu]),
    ]
}

/// sidevoice-engine#4: the builds that run rank by the class of the accelerator each would run on here, a GPU, then a
/// neural engine, then a CPU; within a class in catalogue order; then the builds that do not run, in catalogue order.
#[test]
fn the_builds_that_run_rank_gpu_then_neural_engine_then_cpu() {
    let apple_silicon = caps(
        &[Accelerator::Metal, Accelerator::CoreMl, Accelerator::Cpu],
        Some(16_384),
        Some(8),
    );
    let at = |id: &str, accelerator| (id.to_owned(), accelerator);
    assert_eq!(
        order(NATIVE, &native_backends(), &apple_silicon),
        [
            at("model/whisper-cpp", Some(Accelerator::Metal)),
            at("model/sherpa-onnx", Some(Accelerator::CoreMl)),
            at("model/onnxruntime", Some(Accelerator::Cpu)),
            at("model/sherpa-onnx-cpu", Some(Accelerator::Cpu)),
            at("model/mlx", None),
        ]
    );
}

/// The fallback: where every build that runs is on the CPU (a host with no GPU and no neural engine, as Linux and
/// Windows are to whisper.cpp today), catalogue order stands.
#[test]
fn where_every_build_runs_on_the_cpu_catalogue_order_stands() {
    let cpu_only = caps(&[Accelerator::Cpu], Some(16_384), Some(8));
    let at = |id: &str| (id.to_owned(), Some(Accelerator::Cpu));
    assert_eq!(
        order(NATIVE, &native_backends(), &cpu_only),
        [
            at("model/onnxruntime"),
            at("model/sherpa-onnx-cpu"),
            at("model/sherpa-onnx"),
            at("model/whisper-cpp"),
            ("model/mlx".to_owned(), None),
        ]
    );
}

/// In a page, a build on WebGPU comes before one in WebAssembly, whatever the catalogue's order; without WebGPU, the
/// build that requires it does not run.
#[test]
fn in_a_page_webgpu_ranks_before_webassembly() {
    const PAGE: &[(&str, &str, &[Accelerator])] = &[
        (
            "model/transformers-js-q8",
            "transformers-js",
            &[Accelerator::Wasm],
        ),
        (
            "model/transformers-js-fp16",
            "transformers-js",
            &[Accelerator::WebGpu],
        ),
    ];
    let backends = || {
        vec![running_on(
            "transformers-js",
            &[Accelerator::WebGpu, Accelerator::Wasm],
        )]
    };
    let page = |accelerators: &[Accelerator]| Capabilities {
        runs: Runs::Page,
        os: "web".to_owned(),
        arch: "wasm32".to_owned(),
        accelerators: accelerators.to_vec(),
        memory_mb: Some(16_384),
        cores: Some(8),
    };
    let at = |id: &str, accelerator| (id.to_owned(), accelerator);
    assert_eq!(
        order(
            PAGE,
            &backends(),
            &page(&[Accelerator::Wasm, Accelerator::WebGpu])
        ),
        [
            at("model/transformers-js-fp16", Some(Accelerator::WebGpu)),
            at("model/transformers-js-q8", Some(Accelerator::Wasm)),
        ]
    );
    assert_eq!(
        order(PAGE, &backends(), &page(&[Accelerator::Wasm])),
        [
            at("model/transformers-js-q8", Some(Accelerator::Wasm)),
            at("model/transformers-js-fp16", None),
        ]
    );
}

/// A CUDA build ranks with the GPUs: before a CPU build listed ahead of it.
#[test]
fn cuda_ranks_as_a_gpu() {
    const LINUX: &[(&str, &str, &[Accelerator])] = &[
        ("model/sherpa-onnx", "sherpa-onnx", &[]),
        ("model/whisper-cpp", "whisper-cpp", &[]),
    ];
    let backends = vec![
        running_on("sherpa-onnx", &[Accelerator::Cpu]),
        running_on("whisper-cpp", &[Accelerator::Cuda, Accelerator::Cpu]),
    ];
    let with_cuda = caps(
        &[Accelerator::Cuda, Accelerator::Cpu],
        Some(16_384),
        Some(8),
    );
    let at = |id: &str, accelerator| (id.to_owned(), Some(accelerator));
    assert_eq!(
        order(LINUX, &backends, &with_cuda),
        [
            at("model/whisper-cpp", Accelerator::Cuda),
            at("model/sherpa-onnx", Accelerator::Cpu),
        ]
    );
}
