//! What the engine recommends here: for each local model, the build to run it as, and for speech to text and text to
//! speech together, the model to start with (sidevoice-engine#124, sidevoice-core#33). Pure functions of what the
//! local catalogue lists here (each build's `available` and `reasons`, the resolver's word on whether it runs here,
//! is the only "runs here" check), the catalogue's hand-written lists ([`RecommendedLists`]), the memory the host
//! reports and the person's language. Nothing is downloaded or chosen for the person: the app shows the
//! recommendation and preselects it.
//!
//! The build of a model: of those that run here, ranked by the resolver (a GPU, then a neural engine, then a CPU;
//! catalogue order within a class), the first that does not stop in the background
//! ([`ModelBuild::stops_in_background`]) and fits the memory budget on its own; failing that, the first that does not
//! stop in the background; failing that, the first that runs.
//!
//! The models: each capability's list is walked best first, keeping the entries whose model has a recommended build
//! that runs here, does not stop in the background, runs on the class the entry asks for (if any) and fits the budget
//! on its own (hard: what cannot run well here is never recommended), then, of those, the ones that speak the person's
//! language (soft: a model that does not is still selectable, only not recommended; when none does, the hard list
//! stands, a model rather than none). The pair is the (speech to text i, text to speech j) closest to the top of both
//! lists whose memory, with the end-of-turn model's, fits the budget together: the least i + j, the better speech to
//! text on a tie. The models of a call are loaded together, so the budget is the whole set's: [`BUDGET_PERCENT`] of
//! the memory the host reports (unknown memory sets no budget). When no pair fits, the lightest.

use crate::catalog::{Capability, ListEntry, RecommendedLists};
use crate::engine::{LocalModelInfo, ModelBuild};
use crate::host::{Accelerator, Capabilities};

#[cfg(test)]
mod tests;

/// The share of the memory the host reports that the models of a call (speech to text, text to speech and end of
/// turn, loaded together) may take, in percent: decided at 60 (2026-11-10), to settle in review (60 or 70).
pub(crate) const BUDGET_PERCENT: u32 = 60;

/// The language the person speaks, per capability, as BCP 47 tags: what speech to text hears (the one set for it, else
/// the system's) and what text to speech says (the system's: it has no setting of its own today). The app says which;
/// the engine picks no language. `None`: no language to prefer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Languages {
    /// What speech to text hears.
    pub stt: Option<String>,
    /// What text to speech says.
    pub tts: Option<String>,
}

/// What the engine recommends here for speech to text and for text to speech.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recommendation {
    /// Speech to text.
    pub stt: Recommended,
    /// Text to speech.
    pub tts: Recommended,
}

/// What the engine recommends here for one capability.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recommended {
    /// The model to start with and its build: of [`Recommended::ranked`], the one that fits the memory budget with
    /// the other capability's choice; `None` when no model of the list runs here.
    pub choice: Option<Pick>,
    /// Every model of the capability's list that can be recommended here, best first, each with its build: those
    /// that run here as the list asks and fit the budget on their own, and of those, the ones that speak the person's
    /// language when any does. A family's first here is that family's best for this device.
    pub ranked: Vec<Pick>,
}

/// A model and the build to run it as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    /// The model's id.
    pub model: String,
    /// The build's id.
    pub build: String,
}

/// The memory the models of a call may take here, in MB: [`BUDGET_PERCENT`] of what the host reports; `None` when it
/// cannot tell, which sets no limit.
pub(crate) fn budget_mb(caps: &Capabilities) -> Option<u32> {
    caps.memory_mb.map(|mb| {
        u32::try_from(u64::from(mb) * u64::from(BUDGET_PERCENT) / 100).unwrap_or(u32::MAX)
    })
}

/// Of `builds`, ranked as the resolver ranks them (those that run here first), the one the engine recommends: the
/// first that runs here (`runs_on`), does not stop in the background (`stops`) and takes no more than `budget` MB
/// (`memory_mb`); else the first that runs and does not stop; else the first that runs; `None` when none runs here.
pub(crate) fn build<T>(
    builds: impl IntoIterator<Item = T>,
    runs_on: impl Fn(&T) -> Option<Accelerator>,
    stops: impl Fn(&T) -> bool,
    memory_mb: impl Fn(&T) -> u32,
    budget: Option<u32>,
) -> Option<T> {
    let running: Vec<T> = builds
        .into_iter()
        .filter(|build| runs_on(build).is_some())
        .collect();
    let fits = |build: &T| budget.is_none_or(|budget| memory_mb(build) <= budget);
    let chosen = running
        .iter()
        .position(|build| !stops(build) && fits(build))
        .or_else(|| running.iter().position(|build| !stops(build)))
        .unwrap_or(0);
    running.into_iter().nth(chosen)
}

/// What the engine recommends here for speech to text and text to speech, from the local catalogue's `models` as
/// listed here (each with its recommended build), its `lists`, the host's `caps` and the person's `languages`.
pub(crate) fn recommend(
    models: &[LocalModelInfo],
    lists: &RecommendedLists,
    caps: &Capabilities,
    languages: &Languages,
) -> Recommendation {
    let budget = budget_mb(caps);
    let stt = ranked(
        models,
        &lists.stt,
        Capability::Stt,
        budget,
        languages.stt.as_deref(),
    );
    let tts = ranked(
        models,
        &lists.tts,
        Capability::Tts,
        budget,
        languages.tts.as_deref(),
    );
    // The end-of-turn model a call takes when none is chosen: the first that runs here, as the app picks it.
    let turn = models
        .iter()
        .filter(|model| model.capabilities.contains(&Capability::EndOfTurn))
        .find_map(recommended_build)
        .map_or(0, |build| build.memory_mb);
    let (stt_choice, tts_choice) = pair(&stt, &tts, turn, budget);
    let picks = |candidates: &[Candidate<'_>]| candidates.iter().map(Candidate::pick).collect();
    Recommendation {
        stt: Recommended {
            choice: stt_choice.map(Candidate::pick),
            ranked: picks(&stt),
        },
        tts: Recommended {
            choice: tts_choice.map(Candidate::pick),
            ranked: picks(&tts),
        },
    }
}

/// A model of a list that can be recommended here, with its build.
struct Candidate<'a> {
    model: &'a LocalModelInfo,
    build: &'a ModelBuild,
}

impl Candidate<'_> {
    fn pick(&self) -> Pick {
        Pick {
            model: self.model.id.clone(),
            build: self.build.id.clone(),
        }
    }
}

/// The entries of `list` that can be recommended here (see the module), best first.
fn ranked<'a>(
    models: &'a [LocalModelInfo],
    list: &[ListEntry],
    capability: Capability,
    budget: Option<u32>,
    language: Option<&str>,
) -> Vec<Candidate<'a>> {
    let runnable: Vec<Candidate<'a>> = list
        .iter()
        .filter_map(|entry| {
            let model = models.iter().find(|model| {
                model.id == entry.model && model.capabilities.contains(&capability)
            })?;
            let build = recommended_build(model)?;
            let class = build.accelerator_class()?;
            let fits = budget.is_none_or(|budget| build.memory_mb <= budget);
            let wanted = entry.on.is_none_or(|on| on == class);
            (!build.stops_in_background && wanted && fits).then_some(Candidate { model, build })
        })
        .collect();
    let Some(language) = language else {
        return runnable;
    };
    if !runnable
        .iter()
        .any(|candidate| speaks(candidate.model, language))
    {
        return runnable;
    }
    runnable
        .into_iter()
        .filter(|candidate| speaks(candidate.model, language))
        .collect()
}

/// The build `model` is listed with as recommended here, when it runs here.
fn recommended_build(model: &LocalModelInfo) -> Option<&ModelBuild> {
    let id = model.recommended_build.as_deref()?;
    model
        .builds
        .iter()
        .find(|build| build.id == id && build.available)
}

/// Whether `model` handles `language`: one of its languages or of its voices' has the same primary subtag ("es-ES"
/// handles "es"). A model that names no language says nothing against any.
fn speaks(model: &LocalModelInfo, language: &str) -> bool {
    let primary = |tag: &str| {
        tag.split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let wanted = primary(language);
    let mut tags = model
        .languages
        .iter()
        .chain(model.voices.iter().flat_map(|voice| &voice.languages))
        .peekable();
    tags.peek().is_none() || tags.any(|tag| primary(tag) == wanted)
}

/// The (speech to text, text to speech) of `stt` and `tts` closest to the top of both whose memory, with `turn` MB for
/// the end-of-turn model, fits `budget`: the least sum of places, the better speech to text on a tie. When no pair
/// fits, the lightest; a capability with nothing to recommend leaves the other alone.
fn pair<'a, 'b>(
    stt: &'b [Candidate<'a>],
    tts: &'b [Candidate<'a>],
    turn: u32,
    budget: Option<u32>,
) -> (Option<&'b Candidate<'a>>, Option<&'b Candidate<'a>>) {
    let memory = |candidate: Option<&Candidate<'_>>| candidate.map_or(0, |c| c.build.memory_mb);
    let options = |list: &'b [Candidate<'a>]| -> Vec<(usize, Option<&'b Candidate<'a>>)> {
        if list.is_empty() {
            vec![(0, None)]
        } else {
            list.iter().enumerate().map(|(i, c)| (i, Some(c))).collect()
        }
    };
    let mut pairs = Vec::new();
    for (i, s) in options(stt) {
        for (j, t) in options(tts) {
            let total = memory(s).saturating_add(memory(t)).saturating_add(turn);
            pairs.push((i, j, total, s, t));
        }
    }
    let fits = pairs
        .iter()
        .filter(|(_, _, total, _, _)| budget.is_none_or(|budget| *total <= budget))
        .min_by_key(|(i, j, _, _, _)| (i + j, *i));
    let chosen = fits.or_else(|| {
        pairs
            .iter()
            .min_by_key(|(i, j, total, _, _)| (*total, i + j, *i))
    });
    chosen.map_or((None, None), |(_, _, _, s, t)| (*s, *t))
}
