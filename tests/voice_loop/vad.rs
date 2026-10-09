//! How the loop judges a voice activity detector on a recorded clip: the clip is set between two stretches of digital
//! silence, fed to a stream in small pieces as a microphone would feed it, and what the stream reports must be the
//! clip's speech: every segment inside the clip (give or take the plan's tolerance, since a detector pads speech and
//! clips start and end with a little silence of their own), ended by the silence after it rather than by the end of
//! the audio, and together covering enough of the clip. It checks where speech is, not how well: the clips are not
//! annotated, so their own pauses are not judged.

use std::ops::Range;

/// The plan's `vad`: the builds, and the rule.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VadPlan {
    /// Catalogue builds of voice activity detectors.
    pub(crate) builds: Vec<String>,
    /// The silence before and after each clip, in seconds: longer than a detector needs to end speech.
    pub(crate) silence_s: f64,
    /// How far outside the clip a segment may reach, in seconds.
    pub(crate) tolerance_s: f64,
    /// How much of the clip the segments must cover together, from 0 to 1.
    pub(crate) min_coverage: f64,
}

/// `clip` at `rate`, between `silence_s` of silence on each side, and where the clip lies in it, in seconds.
pub(crate) fn padded(clip: &[f32], rate: u32, silence_s: f64) -> (Vec<f32>, Range<f64>) {
    let silence = vec![0.0; (silence_s * f64::from(rate)) as usize];
    let samples = [&silence[..], clip, &silence[..]].concat();
    let seconds = clip.len() as f64 / f64::from(rate);
    (samples, silence_s..silence_s + seconds)
}

/// Whether `segments` (in seconds, as the stream reported them, the last one perhaps by finishing) are `clip`'s speech
/// by the plan's rule; why not otherwise.
pub(crate) fn judge(
    segments: &[Range<f64>],
    finished: bool,
    clip: &Range<f64>,
    plan: &VadPlan,
) -> Result<(), String> {
    if segments.is_empty() {
        return Err("no speech found".into());
    }
    if finished {
        return Err("the speech had not ended when the audio did".into());
    }
    let (from, to) = (clip.start - plan.tolerance_s, clip.end + plan.tolerance_s);
    if let Some(outside) = segments
        .iter()
        .find(|segment| segment.start < from || segment.end > to)
    {
        return Err(format!(
            "speech at {:.2}–{:.2} s, outside the clip ({:.2}–{:.2} s)",
            outside.start, outside.end, clip.start, clip.end
        ));
    }
    let covered: f64 = segments
        .iter()
        .map(|segment| segment.end.min(clip.end) - segment.start.max(clip.start))
        .filter(|seconds| *seconds > 0.0)
        .sum();
    let coverage = covered / (clip.end - clip.start);
    if coverage < plan.min_coverage {
        return Err(format!(
            "speech covers {:.0}% of the clip, under {:.0}%",
            coverage * 100.0,
            plan.min_coverage * 100.0
        ));
    }
    Ok(())
}

/// The table of detections, `(pair, clip, segments, verdict)` each, with a closing line, and how many failed.
pub(crate) fn table<'a>(
    detections: impl Iterator<
        Item = (
            &'a str,
            &'a Range<f64>,
            &'a [Range<f64>],
            &'a Result<(), String>,
        ),
    >,
) -> (String, usize) {
    let mut table = String::from(
        "| Clip → detector | Clip at (s) | Speech found (s) | Verdict |\n|---|---|---|---|\n",
    );
    let (mut all, mut failed) = (0, 0);
    for (pair, clip, segments, verdict) in detections {
        all += 1;
        failed += usize::from(verdict.is_err());
        let found: Vec<String> = segments
            .iter()
            .map(|segment| format!("{:.2}–{:.2}", segment.start, segment.end))
            .collect();
        let verdict = verdict
            .as_ref()
            .map_or_else(|why| format!("✗ {why}"), |()| "ok".into());
        table.push_str(&format!(
            "| {pair} | {:.2}–{:.2} | {} | {verdict} |\n",
            clip.start,
            clip.end,
            found.join(", ")
        ));
    }
    table.push_str(&format!(
        "\n{} of {all} clips found where they are, between silences.\n",
        all - failed
    ));
    (table, failed)
}
