//! An end-of-turn classifier over a fake model: what it is given (the last seconds it hears, at 16 kHz) and what it
//! may answer.

use std::sync::{Arc, Mutex};

use crate::backend::{BackendModel, EndOfTurnModel, Library};
use crate::catalog::BuildEntry;
use crate::engine::loaded::Resident;
use crate::install::Installed;
use crate::test_support::block_on;
use crate::{async_trait, Accelerator, Capability, Error, LoadedModel, Result};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// A classifier that hears 2 s, answers `answer`, and keeps the length of what it was given.
struct FakeClassifier {
    answer: f32,
    heard: Arc<Mutex<Vec<usize>>>,
}

impl BackendModel for FakeClassifier {
    fn as_end_of_turn(&mut self) -> Option<&mut dyn EndOfTurnModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl EndOfTurnModel for FakeClassifier {
    fn seconds(&self) -> u32 {
        2
    }

    async fn probability(&mut self, pcm: &[f32]) -> Result<f32> {
        self.heard.lock().expect("not poisoned").push(pcm.len());
        Ok(self.answer)
    }
}

struct NoLibrary;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for NoLibrary {
    async fn load(
        &self,
        _build: &BuildEntry,
        _accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn BackendModel>> {
        Err(Error::new("not-implemented"))
    }
}

fn loaded(answer: f32) -> (LoadedModel, Arc<Mutex<Vec<usize>>>) {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let model = Box::new(FakeClassifier {
        answer,
        heard: Arc::clone(&heard),
    });
    let resident = Resident::new(model, Arc::new(NoLibrary), Vec::new(), Vec::new());
    (LoadedModel::new("turn", "turn/fake", resident), heard)
}

#[test]
fn it_is_an_end_of_turn_classifier_only_and_says_what_it_hears() {
    let (loaded, _) = loaded(0.5);
    assert_eq!(loaded.capabilities(), [Capability::EndOfTurn]);
    assert!(loaded.as_stt().is_none() && loaded.as_vad().is_none());
    assert_eq!(loaded.as_end_of_turn().expect("a classifier").seconds(), 2);
}

#[test]
fn only_the_last_seconds_reach_the_model_at_16_khz() {
    let (loaded, heard) = loaded(0.75);
    let classifier = loaded.as_end_of_turn().expect("a classifier");
    // 5 s at 48 kHz: the last 2 s, resampled, are 32 000 samples.
    let p = block_on(classifier.probability(&vec![0.1; 240_000], 48_000));
    assert_eq!(p, Ok(0.75));
    // 1 s at 16 kHz: all of it.
    block_on(classifier.probability(&vec![0.1; 16_000], 16_000)).expect("a probability");
    let heard = heard.lock().expect("not poisoned").clone();
    assert!(heard[0].abs_diff(32_000) <= 1, "{heard:?}");
    assert_eq!(heard[1], 16_000);
}

#[test]
fn an_answer_that_is_not_a_probability_fails() {
    for answer in [1.5, -0.1, f32::NAN] {
        let (loaded, _) = loaded(answer);
        let classifier = loaded.as_end_of_turn().expect("a classifier");
        let failed = block_on(classifier.probability(&[0.0; 160], 16_000));
        assert_eq!(
            failed.map_err(|e| e.code),
            Err("end-of-turn-failed"),
            "{answer}"
        );
    }
}
