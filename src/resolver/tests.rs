//! The funnel's accelerator and requirement steps, on hosts that report little: what the host cannot tell passes, what
//! it does not report is absent, and a probe only narrows what it reports.

use async_trait::async_trait;

use super::Resolver;
use crate::backend::{Backend, BackendSpec, LoadedModel, MinCores};
use crate::catalog::{Catalog, CatalogFragment, CatalogSource};
use crate::install::Installed;
use crate::test_support::{build, family, model};
use crate::{
    Accelerator, Build, Capabilities, Capability, Error, Offer, Reason, Rejection, Result, Runs,
};

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

    async fn load(
        &self,
        _build: &Build,
        _accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn LoadedModel>> {
        Err(Error::new("not-implemented"))
    }
}

/// A catalogue of one model with one build for that backend that needs 2 GB.
struct OneBuildCatalog;

impl CatalogSource for OneBuildCatalog {
    fn load(&self) -> Result<CatalogFragment> {
        let builds = vec![build("model/sherpa-onnx", "sherpa-onnx", 2_048)];
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

/// The accelerator the one build is offered on, or why it is rejected.
fn fit(probe: &'static [Accelerator], caps: &Capabilities) -> Result<Accelerator, Rejection> {
    let catalog =
        Catalog::merge(&[Box::new(OneBuildCatalog) as Box<dyn CatalogSource>]).expect("catalogue");
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
