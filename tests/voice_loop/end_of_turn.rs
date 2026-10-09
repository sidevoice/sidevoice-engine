//! How the loop judges an end-of-turn model on a recorded clip. Each clip is heard twice, each time followed by the
//! same short pause, the moment a silence-based detector would end the turn: whole, where the speaker has finished,
//! and cut mid-phrase, where they have not. The cut is the middle of the clip's longest stretch of speech without a
//! pause (100 ms windows each louder than the plan's `pause_floor`, a fraction of the loudest window): words go on
//! on both sides of it, so it is never the end of a phrase, which may be a sentence of its own. The model must say the whole clip is a complete turn and the cut one is not:
//! it ends turns where silence alone would have cut too early.

/// The plan's `end_of_turn`: the builds, and the rule.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndOfTurnPlan {
    /// Catalogue builds of end-of-turn models.
    pub(crate) builds: Vec<String>,
    /// The silence after the speech, in seconds: the pause a detector would end the turn at.
    pub(crate) pause_s: f64,
    /// Below this fraction of the loudest 100 ms, a 100 ms window is a pause.
    pub(crate) pause_floor: f32,
    /// The probability at or above which a turn is complete.
    pub(crate) threshold: f32,
}

/// The clip `samples` at `rate`, whole and cut, each followed by the plan's pause.
pub(crate) fn heard(samples: &[f32], rate: u32, plan: &EndOfTurnPlan) -> (Vec<f32>, Vec<f32>) {
    let pause = vec![0.0; (plan.pause_s * f64::from(rate)) as usize];
    let cut = cut_point(samples, rate, plan.pause_floor);
    (
        [samples, &pause].concat(),
        [&samples[..cut], &pause[..]].concat(),
    )
}

/// Whether the whole clip's probability says complete and the cut one's does not; why not otherwise.
pub(crate) fn judge(whole: f32, cut: f32, plan: &EndOfTurnPlan) -> Result<(), String> {
    match (whole >= plan.threshold, cut < plan.threshold) {
        (true, true) => Ok(()),
        (false, _) => Err(format!(
            "the whole clip is not a complete turn ({whole:.2} < {:.2})",
            plan.threshold
        )),
        (true, false) => Err(format!(
            "the cut clip is a complete turn ({cut:.2} ≥ {:.2})",
            plan.threshold
        )),
    }
}

/// One clip heard by one end-of-turn model: the probabilities it gave the whole and the cut clip, and the verdict.
pub(crate) struct Turn {
    pub(crate) pair: String,
    pub(crate) whole: Option<f32>,
    pub(crate) cut: Option<f32>,
    pub(crate) verdict: Result<(), String>,
}

/// The table of turns, with a closing line, and how many failed.
pub(crate) fn table(turns: &[Turn]) -> (String, usize) {
    let mut table = String::from(
        "| Clip → end-of-turn model | P(complete), whole | P(complete), cut | Verdict |\n|---|---|---|---|\n",
    );
    let probability = |p: Option<f32>| p.map_or_else(|| "–".to_owned(), |p| format!("{p:.2}"));
    let mut failed = 0;
    for turn in turns {
        failed += usize::from(turn.verdict.is_err());
        let verdict = turn
            .verdict
            .as_ref()
            .map_or_else(|why| format!("✗ {why}"), |()| "ok".into());
        table.push_str(&format!(
            "| {} | {} | {} | {verdict} |\n",
            turn.pair,
            probability(turn.whole),
            probability(turn.cut)
        ));
    }
    table.push_str(&format!(
        "\n{} of {} clips: complete whole, not complete cut.\n",
        turns.len() - failed,
        turns.len()
    ));
    (table, failed)
}

/// Where `samples` (at `rate`) is cut: the middle of its longest run of 100 ms windows each louder than `floor` times
/// the loudest window.
pub(crate) fn cut_point(samples: &[f32], rate: u32, floor: f32) -> usize {
    let window = (rate / 10).max(1) as usize;
    let energies: Vec<f32> = samples
        .chunks_exact(window)
        .map(|chunk| chunk.iter().map(|sample| sample * sample).sum())
        .collect();
    let floor = energies.iter().copied().fold(0.0, f32::max) * floor;
    let (mut longest, mut start) = (0..0, None);
    for (at, energy) in energies.iter().chain([&0.0]).enumerate() {
        match (*energy > floor, start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                if at - from > longest.len() {
                    longest = from..at;
                }
                start = None;
            }
            _ => {}
        }
    }
    (longest.start + longest.end) * window / 2
}
