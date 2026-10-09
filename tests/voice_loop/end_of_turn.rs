//! How the loop judges an end-of-turn model on a recorded clip. Each clip is heard twice, each time followed by the
//! same short pause, the moment a silence-based detector would end the turn: whole, where the speaker has finished,
//! and cut inside a word, where they have not. The cut is where the clip is loudest (the 20 ms of most energy) within
//! the span the plan gives, so it falls in a voiced sound, never at a pause or between phrases. The model must say the whole clip is a complete turn and the cut one is not:
//! it ends turns where silence alone would have cut too early.

/// The plan's `end_of_turn`: the builds, and the rule.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndOfTurnPlan {
    /// Catalogue builds of end-of-turn models.
    pub(crate) builds: Vec<String>,
    /// The silence after the speech, in seconds: the pause a detector would end the turn at.
    pub(crate) pause_s: f64,
    /// Where the clip may be cut for its unfinished version, as fractions of its length: the loudest 20 ms in there.
    pub(crate) cut_within: [f64; 2],
    /// The probability at or above which a turn is complete.
    pub(crate) threshold: f32,
}

/// The clip `samples` at `rate`, whole and cut, each followed by the plan's pause.
pub(crate) fn heard(samples: &[f32], rate: u32, plan: &EndOfTurnPlan) -> (Vec<f32>, Vec<f32>) {
    let pause = vec![0.0; (plan.pause_s * f64::from(rate)) as usize];
    let cut = cut_point(samples, rate, plan.cut_within);
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

/// Where `samples` (at `rate`) is cut: the middle of its loudest 20 ms between the fractions `within` of its length.
pub(crate) fn cut_point(samples: &[f32], rate: u32, within: [f64; 2]) -> usize {
    let frame = (rate / 50).max(1) as usize;
    let from = (samples.len() as f64 * within[0]) as usize / frame;
    let to = ((samples.len() as f64 * within[1]) as usize / frame).max(from + 1);
    let energy = |at: usize| -> f32 {
        let window = samples
            .get(at * frame..(at + 1) * frame)
            .unwrap_or_default();
        window.iter().map(|sample| sample * sample).sum()
    };
    let loudest = (from..to)
        .max_by(|a, b| energy(*a).total_cmp(&energy(*b)))
        .unwrap_or(from);
    (loudest * frame + frame / 2).min(samples.len())
}
