//! The speeds a text-to-speech model takes, as its source states them: [`SpeedRange`].

use serde::Deserialize;

/// The speeds a model takes (1 is its normal pace), and where that comes from. A model with none does not take a
/// speed at all. A bound is there only where the source states it: a model that takes a speed whose source publishes no
/// limits has neither (Piper), and nothing is guessed.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeedRange {
    /// The slowest speed the source gives, if it gives one.
    #[serde(default)]
    pub min: Option<f32>,
    /// The fastest speed the source gives, if it gives one.
    #[serde(default)]
    pub max: Option<f32>,
    /// Where the range comes from: a pinned link to what states it (a local family's publisher, a remote provider's
    /// spec).
    pub source: String,
}

impl SpeedRange {
    /// Whether it says something coherent: positive bounds, the slowest no faster than the fastest, and a source.
    pub(crate) fn is_valid(&self) -> bool {
        let positive =
            |bound: Option<f32>| bound.is_none_or(|bound| bound.is_finite() && bound > 0.0);
        let ordered = match (self.min, self.max) {
            (Some(min), Some(max)) => min <= max,
            _ => true,
        };
        positive(self.min) && positive(self.max) && ordered && !self.source.trim().is_empty()
    }
}
