//! A loaded model as an end-of-turn classifier: [`EndOfTurn`], which says how likely it is that a turn is complete.

use super::{audio, LoadedModel};
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// The rate every end-of-turn backend takes (`EndOfTurnModel::probability`).
const RATE: u32 = 16_000;

/// A loaded model, as an end-of-turn classifier.
#[derive(Debug, Clone, Copy)]
pub struct EndOfTurn<'a>(pub(super) &'a LoadedModel);

impl EndOfTurn<'_> {
    /// How many seconds of the end of a turn the model hears (smart-turn: 8): audio before them does not count, so a
    /// caller may pass only those.
    #[must_use]
    pub fn seconds(&self) -> u32 {
        self.0.resident.end_of_turn_seconds.unwrap_or_default()
    }

    /// The probability, from 0 to 1, that the turn in `audio` (mono samples at `sample_rate` Hz, from the turn's start
    /// to now, typically at a pause the voice activity detector found) is complete. The engine keeps the last
    /// [`EndOfTurn::seconds`] and brings them to the model's rate. It runs on the calling task: the app decides where.
    ///
    /// # Errors
    ///
    /// The backend's `end-of-turn-failed`, which is also what a model that answers something other than a probability
    /// fails with.
    pub async fn probability(&self, audio: &[f32], sample_rate: u32) -> Result<f32> {
        let kept = (u64::from(self.seconds()) * u64::from(sample_rate)) as usize + 1;
        let tail = &audio[audio.len().saturating_sub(kept)..];
        let pcm = audio::resample(tail, sample_rate, RATE);
        let mut model = self.0.resident.model.lock().await;
        let classifier = model
            .as_end_of_turn()
            .ok_or(Error::new("model-cannot-end-turns"))?;
        let probability = classifier.probability(&pcm).await?;
        if (0.0..=1.0).contains(&probability) {
            Ok(probability)
        } else {
            Err(Error::new("end-of-turn-failed"))
        }
    }
}
