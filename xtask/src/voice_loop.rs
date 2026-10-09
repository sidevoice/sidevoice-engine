//! What the web voice loop (`cargo xtask web-e2e`) takes from the native one (`tests/voice_loop.rs`, the engine's
//! integration test): the real recorded clips of its plan, `tests/voice_loop.json`, downloaded once and checked
//! against their digests, and the table of comparisons, each transcript held to a word error rate (`wer.rs`, the same
//! measure as `tests/voice_loop/wer.rs`: xtask is a package of its own, so it keeps its copy), and the rule a voice
//! activity detector is judged by on those clips (the same as `tests/voice_loop/vad.rs`, a copy for the same reason).

use std::ops::Range;
use std::path::Path;
use std::{env, fs};

use serde::Deserialize;

use crate::{read, run_in, sha256, write, Result};

#[cfg(test)]
mod tests;

/// A real recording, with what is said in it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Clip {
    /// Its primary language subtag.
    pub(crate) language: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    /// What is said in it, as its source transcribes it.
    pub(crate) text: String,
    /// Where it comes from, and its licence: for people, not read.
    pub(crate) source: String,
    pub(crate) license: String,
}

/// One comparison: who said it to whom, in which language, what was said and what was heard.
pub(crate) struct Row {
    pub(crate) pair: String,
    pub(crate) language: String,
    pub(crate) said: String,
    pub(crate) heard: Result<String>,
}

/// The clip, from `clips` or downloaded into it, checked against its digest either way.
pub(crate) fn fetch(clips: &Path, clip: &Clip) -> Result<Vec<u8>> {
    let path = clips.join(&clip.sha256);
    if !path.is_file() {
        let partial = clips.join(format!("{}.partial", clip.sha256));
        let target = partial.to_string_lossy();
        run_in(clips, "curl -fsSL --retry 3 -o", &[&target, &clip.url])?;
        fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let bytes = read(&path)?;
    let digest = sha256(&bytes);
    if digest != clip.sha256 {
        let _ = fs::remove_file(&path);
        return Err(format!(
            "{}: sha256 {digest}, not {}",
            clip.url, clip.sha256
        ));
    }
    Ok(bytes)
}

/// Prints the table, writes it to `$GITHUB_STEP_SUMMARY` when set, and fails if any row failed or is above `max_wer`.
pub(crate) fn report(title: &str, rows: &[Row], max_wer: f64) -> Result<()> {
    let mut table =
        String::from("| Model pair | Language | Expected | Got | WER |\n|---|---|---|---|---|\n");
    let mut bad = 0;
    for row in rows {
        let (got, wer, ok) = match &row.heard {
            Ok(heard) => {
                let wer = crate::wer::wer(&row.said, heard);
                let mark = if wer <= max_wer { "" } else { " ✗" };
                (
                    heard.clone(),
                    format!("{:.0}%{mark}", wer * 100.0),
                    wer <= max_wer,
                )
            }
            Err(code) => (format!("`{code}`"), "✗".to_owned(), false),
        };
        bad += usize::from(!ok);
        let cell = |text: &str| text.replace('|', "\\|");
        table.push_str(&format!(
            "| {} | {} | {} | {} | {wer} |\n",
            cell(&row.pair),
            row.language,
            cell(&row.said),
            cell(&got)
        ));
    }
    let verdict = format!(
        "{} of {} within {:.0}% WER (normalised: lower case, no punctuation, vowel accents folded).",
        rows.len() - bad,
        rows.len(),
        max_wer * 100.0
    );
    println!("\n{table}\n{verdict}");
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text =
            String::from_utf8_lossy(&fs::read(&summary).unwrap_or_default()).into_owned();
        text.push_str(&format!("## {title}\n\n{table}\n{verdict}\n"));
        write(Path::new(&summary), text.as_bytes())?;
    }
    if bad > 0 || rows.is_empty() {
        return Err(format!("{bad} of {} comparisons failed", rows.len()));
    }
    Ok(())
}

/// The primary language subtag of the BCP 47 tag `tag`, lower-cased.
pub(crate) fn primary(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// How a voice activity detector is judged on a clip set between two stretches of silence: the plan's `vad`, less its
/// builds (the native ones).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct VadRule {
    /// The silence before and after each clip, in seconds.
    pub(crate) silence_s: f64,
    /// How far outside the clip a segment may reach, in seconds.
    pub(crate) tolerance_s: f64,
    /// How much of the clip the segments must cover together, from 0 to 1.
    pub(crate) min_coverage: f64,
}

/// One clip heard by one detector: where the clip lies, the speech found (seconds), whether its last segment was ended by
/// the end of the audio, how many speech starts were reported, or why it could not run.
pub(crate) struct Detection {
    pub(crate) pair: String,
    pub(crate) clip: Range<f64>,
    pub(crate) segments: Vec<Range<f64>>,
    pub(crate) finished: bool,
    pub(crate) starts: usize,
    pub(crate) error: Option<String>,
}

/// Whether `detection` found the clip's speech there and nowhere else, covering enough of it; why not otherwise.
pub(crate) fn judge(detection: &Detection, rule: &VadRule) -> Result<()> {
    if let Some(error) = &detection.error {
        return Err(format!("`{error}`"));
    }
    let segments = &detection.segments;
    if detection.starts != segments.len() {
        return Err(format!(
            "{} speech starts for {} ends",
            detection.starts,
            segments.len()
        ));
    }
    if segments.is_empty() {
        return Err("no speech found".into());
    }
    if detection.finished {
        return Err("the speech had not ended when the audio did".into());
    }
    let clip = &detection.clip;
    let (from, to) = (clip.start - rule.tolerance_s, clip.end + rule.tolerance_s);
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
    if coverage < rule.min_coverage {
        return Err(format!(
            "speech covers {:.0}% of the clip, under {:.0}%",
            coverage * 100.0,
            rule.min_coverage * 100.0
        ));
    }
    Ok(())
}

/// Prints the table of detections, adds it to `$GITHUB_STEP_SUMMARY` when set, and fails if any detection failed.
pub(crate) fn report_detections(
    title: &str,
    detections: &[Detection],
    rule: &VadRule,
) -> Result<()> {
    let mut table = String::from(
        "| Clip → detector | Clip at (s) | Speech found (s) | Verdict |\n|---|---|---|---|\n",
    );
    let mut failed = 0;
    for detection in detections {
        let verdict = judge(detection, rule);
        failed += usize::from(verdict.is_err());
        let found: Vec<String> = detection
            .segments
            .iter()
            .map(|segment| format!("{:.2}–{:.2}", segment.start, segment.end))
            .collect();
        let verdict = verdict.map_or_else(|why| format!("✗ {why}"), |()| "ok".into());
        table.push_str(&format!(
            "| {} | {:.2}–{:.2} | {} | {verdict} |\n",
            detection.pair,
            detection.clip.start,
            detection.clip.end,
            found.join(", ")
        ));
    }
    let verdict = format!(
        "{} of {} clips found where they are, between silences.",
        detections.len() - failed,
        detections.len()
    );
    println!("\n{table}\n{verdict}");
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text =
            String::from_utf8_lossy(&fs::read(&summary).unwrap_or_default()).into_owned();
        text.push_str(&format!("## {title}\n\n{table}\n{verdict}\n"));
        write(Path::new(&summary), text.as_bytes())?;
    }
    if failed > 0 || detections.is_empty() {
        return Err(format!(
            "{failed} of {} detections failed",
            detections.len()
        ));
    }
    Ok(())
}

/// How an end-of-turn model is judged on a clip heard whole and cut, each followed by a pause: the plan's
/// `end_of_turn`, less its builds (the same rule as `tests/voice_loop/end_of_turn.rs`).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EndOfTurnRule {
    /// The silence after the speech, in seconds.
    pub(crate) pause_s: f64,
    /// Where the clip may be cut, as fractions of its length: its loudest 20 ms in there.
    pub(crate) cut_within: [f64; 2],
    /// The probability at or above which a turn is complete.
    pub(crate) threshold: f32,
}

/// One clip heard by one end-of-turn model: the probabilities of the whole and the cut clip, or why there are none.
pub(crate) struct Turn {
    pub(crate) pair: String,
    pub(crate) whole: Option<f32>,
    pub(crate) cut: Option<f32>,
    pub(crate) error: Option<String>,
}

/// Whether the whole clip is a complete turn and the cut one is not; why not otherwise.
pub(crate) fn judge_turn(turn: &Turn, rule: &EndOfTurnRule) -> Result<()> {
    if let Some(error) = &turn.error {
        return Err(format!("`{error}`"));
    }
    let (Some(whole), Some(cut)) = (turn.whole, turn.cut) else {
        return Err("no probability".into());
    };
    if whole < rule.threshold {
        return Err(format!(
            "the whole clip is not a complete turn ({whole:.2} < {:.2})",
            rule.threshold
        ));
    }
    if cut >= rule.threshold {
        return Err(format!(
            "the cut clip is a complete turn ({cut:.2} ≥ {:.2})",
            rule.threshold
        ));
    }
    Ok(())
}

/// Prints the table of turns, adds it to `$GITHUB_STEP_SUMMARY` when set, and fails if any turn failed.
pub(crate) fn report_turns(title: &str, turns: &[Turn], rule: &EndOfTurnRule) -> Result<()> {
    let mut table = String::from(
        "| Clip → end-of-turn model | P(complete), whole | P(complete), cut | Verdict |\n|---|---|---|---|\n",
    );
    let probability = |p: Option<f32>| p.map_or_else(|| "–".to_owned(), |p| format!("{p:.2}"));
    let mut failed = 0;
    for turn in turns {
        let verdict = judge_turn(turn, rule);
        failed += usize::from(verdict.is_err());
        let verdict = verdict.map_or_else(|why| format!("✗ {why}"), |()| "ok".into());
        table.push_str(&format!(
            "| {} | {} | {} | {verdict} |\n",
            turn.pair,
            probability(turn.whole),
            probability(turn.cut)
        ));
    }
    let verdict = format!(
        "{} of {} clips: complete whole, not complete cut.",
        turns.len() - failed,
        turns.len()
    );
    println!("\n{table}\n{verdict}");
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text =
            String::from_utf8_lossy(&fs::read(&summary).unwrap_or_default()).into_owned();
        text.push_str(&format!("## {title}\n\n{table}\n{verdict}\n"));
        write(Path::new(&summary), text.as_bytes())?;
    }
    if failed > 0 || turns.is_empty() {
        return Err(format!("{failed} of {} turns failed", turns.len()));
    }
    Ok(())
}
