//! `cargo xtask test-browser`: the engine's wasm32 tests in a headless Chrome, the ignored ones included: those that
//! need a page (OPFS, the test server's HTTP answers) and are ignored in Node. wasm-bindgen-test-runner drives Chrome
//! through ChromeDriver, which `CHROMEDRIVER` names (else `chromedriver` on the `PATH`); `CHROME`, when set, is the
//! Chrome it starts, which must be ChromeDriver's version (CI installs both, pinned, with setup-chrome).

use std::env;
use std::process::Command;

use serde_json::json;

use crate::{metadata, repo, write, Result};

pub(crate) fn run() -> Result<()> {
    let (_, target) = metadata()?;
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut test = Command::new(cargo);
    test.current_dir(repo())
        .args([
            "test",
            "--locked",
            "--target",
            "wasm32-unknown-unknown",
            "--lib",
        ])
        .args(["--", "--include-ignored"])
        .env("WASM_BINDGEN_USE_BROWSER", "1");
    // Chrome in a container (CI's runners included) needs its sandbox off.
    let mut options = json!({ "args": ["--no-sandbox"] });
    if let Ok(chrome) = env::var("CHROME") {
        options["binary"] = chrome.into();
    }
    let webdriver = target.join("webdriver.json");
    let capabilities = json!({ "goog:chromeOptions": options });
    write(&webdriver, format!("{capabilities:#}\n").as_bytes())?;
    test.env("WASM_BINDGEN_TEST_WEBDRIVER_JSON", &webdriver);
    let status = test
        .status()
        .map_err(|error| format!("cargo test: {error}"))?;
    if !status.success() {
        return Err(format!("the wasm32 tests in Chrome: {status}"));
    }
    Ok(())
}
