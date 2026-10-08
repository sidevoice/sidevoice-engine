//! The funnel's accelerator and requirement steps, on hosts that report little: what the host cannot tell passes, what
//! it does not report is absent, a probe only narrows what it reports, and a build only runs on what it requires.

use async_trait::async_trait;

use super::Resolver;
use crate::backend::{Backend, BackendSpec, Library, MinCores};
use crate::catalog::{Catalog, CatalogFragment, CatalogSource};
use crate::install::Installed;
use crate::test_support::{build, family, model};
use crate::{Accelerator, Capabilities, Capability, Error, Offer, Reason, Rejection, Result, Runs};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// A backend that runs on CoreML or the CPU, needs 4 cores, and whose probe confirms `probe`. It borrows the id
/// "sherpa-onnx" for its entry in `backends.json` on the test host's platform (linux-x86_64), which the funnel
/// checks first.
struct FixedProbeBackend {
    spec: BackendSpec,
    probe: &'static [Accelerator],
}

impl FixedProbeBackend {
    fn probing(probe: &'static [Accelerator]) -> Box<dyn Backend> {
        Box::new(Self {
            spec: BackendSpec {
                id: "sherpa-onnx",
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
    let catalog = Catalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    let offers = Resolver::default().offers(
        &catalog,
        &[FixedProbeBackend::probing(probe)],
        caps,
        Capability::Stt,
    );
    match <[Offer; 1]>::try_from(offers).expect("one offer") {
        [Offer::Offered { accelerator, .. }] => Ok(accelerator),
        [Offer::Rejected { why, .. }] => Err(why),
    }
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
    let catalog = Catalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    let offers = Resolver::default().offers(
        &catalog,
        &[FixedProbeBackend::probing(COREML_AND_CPU)],
        &caps(COREML_AND_CPU, Some(16_384), Some(8)),
        Capability::Tts,
    );
    let kokoro: Vec<_> = offers
        .iter()
        .filter_map(|offer| match offer {
            Offer::Offered {
                model,
                build,
                accelerator,
                ..
            } if model.id.starts_with("kokoro") && build.backend == "sherpa-onnx" => {
                Some(*accelerator)
            }
            _ => None,
        })
        .collect();
    assert!(!kokoro.is_empty(), "a Kokoro build on sherpa-onnx");
    assert!(kokoro
        .iter()
        .all(|accelerator| *accelerator == Accelerator::Cpu));
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
    let catalog = Catalog::merge(&[Box::new(source) as Box<dyn CatalogSource>]).expect("catalogue");
    let backend = PageBackend(BackendSpec {
        id: "transformers-js",
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
    let offers = Resolver::default().offers(&catalog, &[Box::new(backend)], &caps, Capability::Stt);
    match <[Offer; 1]>::try_from(offers).expect("one offer") {
        [Offer::Offered { accelerator, .. }] => Ok(accelerator),
        [Offer::Rejected { why, .. }] => Err(why),
    }
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
