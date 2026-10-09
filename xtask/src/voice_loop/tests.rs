//! The rule a voice activity detector is judged by on the web, as the native loop judges it.

use super::{judge, Detection, VadRule};

fn detection(segments: &[(f64, f64)], finished: bool) -> Detection {
    Detection {
        pair: "clip → detector".into(),
        clip: 1.0..5.0,
        segments: segments.iter().map(|(start, end)| *start..*end).collect(),
        finished,
        starts: segments.len(),
        error: None,
    }
}

#[test]
fn a_detection_is_the_clips_speech_between_its_silences() {
    let rule = VadRule {
        silence_s: 1.0,
        tolerance_s: 0.3,
        min_coverage: 0.5,
    };
    assert_eq!(
        judge(&detection(&[(1.1, 3.0), (3.5, 4.9)], false), &rule),
        Ok(())
    );
    assert_eq!(judge(&detection(&[(0.8, 4.0)], false), &rule), Ok(()));
    let why = |segments: &[(f64, f64)], finished| {
        judge(&detection(segments, finished), &rule).unwrap_err()
    };
    assert_eq!(why(&[], false), "no speech found");
    assert!(why(&[(0.2, 4.0)], false).contains("outside the clip"));
    assert!(why(&[(1.0, 5.6)], false).contains("outside the clip"));
    assert!(why(&[(1.0, 2.0)], false).contains("covers 25%"));
    assert_eq!(
        why(&[(1.0, 5.0)], true),
        "the speech had not ended when the audio did"
    );
    let unpaired = Detection {
        starts: 2,
        ..detection(&[(1.0, 4.0)], false)
    };
    assert_eq!(
        judge(&unpaired, &rule).unwrap_err(),
        "2 speech starts for 1 ends"
    );
    let failed = Detection {
        error: Some("detection-failed".into()),
        ..detection(&[], false)
    };
    assert_eq!(judge(&failed, &rule).unwrap_err(), "`detection-failed`");
}
