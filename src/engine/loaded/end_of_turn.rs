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
    /// [`EndOfTurn::seconds`] (and, before them, what its resampling filter needs to see) and brings them to the model's
    /// rate, band-limited. It runs on the calling task: the app decides where.
    ///
    /// # Errors
    ///
    /// Checked before the model is asked anything:
    ///
    /// - `invalid-sample-rate`: the sample rate is 0, so the audio has no time base.
    /// - `invalid-audio`: a sample of the audio the model would hear is not a finite number (NaN or infinite).
    ///
    /// And the backend's `end-of-turn-failed`, which is also what a model that answers something other than a
    /// probability fails with.
    pub async fn probability(&self, audio: &[f32], sample_rate: u32) -> Result<f32> {
        if sample_rate == 0 {
            return Err(Error::new("invalid-sample-rate"));
        }
        let heard = (u64::from(self.seconds()) * u64::from(sample_rate)) as usize;
        let kept = heard + audio::context(sample_rate, RATE);
        let tail = &audio[audio.len().saturating_sub(kept)..];
        if tail.iter().any(|sample| !sample.is_finite()) {
            return Err(Error::new("invalid-audio"));
        }
        let resampled = audio::resample(tail, sample_rate, RATE);
        let wanted = (self.seconds() * RATE) as usize;
        let pcm = &resampled[resampled.len().saturating_sub(wanted)..];
        let mut model = self.0.resident.model.lock().await;
        let classifier = model
            .as_end_of_turn()
            .ok_or(Error::new("model-cannot-end-turns"))?;
        let probability = classifier.probability(pcm).await?;
        if (0.0..=1.0).contains(&probability) {
            Ok(probability)
        } else {
            Err(Error::new("end-of-turn-failed"))
        }
    }
}
