//! What both voice loops share (`cargo xtask e2e`, natively, and `cargo xtask web-e2e`, in a browser): the real
//! recorded clips of `xtask/e2e.json`, downloaded once and checked against their digests, and the table of
//! comparisons, each transcript held to a word error rate (`wer.rs`).

use std::path::Path;
use std::{env, fs};

use serde::Deserialize;

use crate::{read, run_in, sha256, write, Result};

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
    /// The highest word error rate it may have, when it is not the plan's.
    pub(crate) max_wer: Option<f64>,
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

/// Prints the table, writes it to `$GITHUB_STEP_SUMMARY` when set, and fails if any row failed or is above its highest
/// word error rate: its own, or `default`.
pub(crate) fn report(title: &str, rows: &[Row], default: f64) -> Result<()> {
    let mut table =
        String::from("| Model pair | Language | Expected | Got | WER |\n|---|---|---|---|---|\n");
    let mut bad = 0;
    for row in rows {
        let max_wer = row.max_wer.unwrap_or(default);
        let (got, wer, ok) = match &row.heard {
            Ok(heard) => {
                let wer = crate::wer::wer(&row.said, heard);
                let own = match row.max_wer {
                    Some(own) => format!(" (≤ {:.0}%)", own * 100.0),
                    None => String::new(),
                };
                let mark = if wer <= max_wer {
                    own
                } else {
                    format!("{own} ✗")
                };
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
        "{} of {} within {:.0}% WER, or the voice's own limit where the plan gives one (normalised: lower case, no \
         punctuation, vowel accents folded).",
        rows.len() - bad,
        rows.len(),
        default * 100.0
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
