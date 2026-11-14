use crate::Error;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn an_error_shows_its_code_and_the_size_of_what_the_provider_said() {
    let error = Error::with_detail(
        "credential-rejected",
        Some("Incorrect API key: sk-canary-***".into()),
    );
    let shown = format!("{error:?}");
    assert!(
        shown.contains("credential-rejected") && shown.contains("<32 chars>"),
        "{shown}"
    );
    assert!(!shown.contains("canary"), "{shown}");
    assert_eq!(error.to_string(), "credential-rejected");
}
