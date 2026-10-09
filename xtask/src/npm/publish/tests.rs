use serde_json::json;

use super::{calling_workflow, dist_tag, sha1_hex, stage_id, staged_version, Staged};

#[test]
fn a_release_goes_to_latest_and_a_candidate_to_next() {
    assert_eq!(dist_tag("0.2.0"), "latest");
    assert_eq!(dist_tag("0.2.0-rc.1"), "next");
}

#[test]
fn the_stage_id_is_read_from_npm_s_report() {
    let report =
        r#"{"@sidevoice/engine": {"id": "@sidevoice/engine@0.3.0", "stageId": "abc-123"}}"#;
    assert_eq!(stage_id(report).as_deref(), Some("abc-123"));
    assert_eq!(stage_id("not json"), None);
}

#[test]
fn a_version_waiting_for_approval_is_found_among_the_staged_ones() {
    let list = json!({"total": 3, "items": [
        {"id": "s-1", "packageName": "@sidevoice/engine", "version": "0.3.0", "status": "rejected", "shasum": "aa"},
        {"id": "s-2", "packageName": "@sidevoice/engine", "version": "0.3.0", "status": "pending", "shasum": "bb"},
        {"id": "s-3", "packageName": "@sidevoice/engine", "version": "0.2.0", "status": "pending", "shasum": "cc"},
    ]});
    let found = staged_version(&list, "0.3.0");
    assert_eq!(
        found,
        Some(Staged {
            id: "s-2".into(),
            shasum: "bb".into()
        })
    );
    assert_eq!(staged_version(&list, "0.4.0"), None);
    assert_eq!(staged_version(&json!({}), "0.3.0"), None);
}

#[test]
fn a_tarball_s_shasum_is_its_sha1() {
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
}

#[test]
fn the_workflow_npm_sees_is_the_calling_one() {
    let workflow = calling_workflow();
    assert!(
        !workflow.contains('@') && !workflow.contains('/'),
        "{workflow}"
    );
}
