//! The speeds a text-to-speech model takes: [`SpeedRange`], what the engine lists, and [`FamilySpeed`], how the
//! catalogue declares it for a family, with where it comes from.

use serde::Deserialize;

/// The speeds a model takes, 1 being its normal pace: from `min` to `max`. A model with none takes no speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedRange {
    /// The slowest.
    pub min: f32,
    /// The fastest.
    pub max: f32,
}

/// A family's speeds, as the catalogue declares them, with their provenance: the `source` that states the range, or,
/// where no source publishes one, the source that the model takes a speed at all, and who `decided_by` the range (the
/// engine's own choice, never presented as the publisher's). The engine lists only the range ([`SpeedRange`]).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilySpeed {
    /// The slowest speed.
    pub min: f32,
    /// The fastest speed.
    pub max: f32,
    /// A pinned link to what states the range, or, for a decided one, to what states that the model takes a speed.
    pub source: String,
    /// Who chose the range, when no source publishes one: `sidevoice`, with why. Absent, the range is the source's.
    #[serde(default)]
    pub decided_by: Option<String>,
}

impl FamilySpeed {
    /// The range the engine lists.
    #[must_use]
    pub fn range(&self) -> SpeedRange {
        SpeedRange {
            min: self.min,
            max: self.max,
        }
    }

    /// Whether it says something coherent: positive bounds, the slowest no faster than the fastest, a source, and a
    /// decision that says who made it.
    pub(crate) fn is_valid(&self) -> bool {
        let positive = |bound: f32| bound.is_finite() && bound > 0.0;
        let named = |text: &str| !text.trim().is_empty();
        positive(self.min)
            && positive(self.max)
            && self.min <= self.max
            && named(&self.source)
            && self.decided_by.as_deref().is_none_or(named)
    }
}
