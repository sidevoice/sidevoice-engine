//! The wasm-bindgen CLI: `wasm-bindgen`, which turns the wasm32 build into the npm package's JavaScript, and
//! `wasm-bindgen-test-runner`, which runs the wasm32 tests in Node. Both must be the version of the `wasm-bindgen`
//! crate in Cargo.lock (the CLI refuses any other), pinned here as [`VERSION`] with the digest of each host's release
//! asset, taken when the version was pinned.
//!
//! A pair of the right version on PATH is used as it is. Otherwise the release asset for this host is downloaded,
//! checked against its pinned digest and unpacked into `target/tools/wasm-bindgen-<version>/`, once.
//!
//! Moving to another version: bump the crate (Cargo.lock), set [`VERSION`] and every digest in [`BUILDS`] to that
//! release's assets.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::*;
use crate::Result;

pub(crate) const VERSION: &str = "0.2.129";
const RELEASES: &str = "https://github.com/wasm-bindgen/wasm-bindgen/releases/download";

/// Per build host (`std::env::consts` OS and ARCH): the release asset's triple and its SHA-256.
const BUILDS: &[(&str, &str, &str, &str)] = &[
    (
        "linux",
        "x86_64",
        "x86_64-unknown-linux-musl",
        "82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e",
    ),
    (
        "linux",
        "aarch64",
        "aarch64-unknown-linux-gnu",
        "1797c349a0f45d30946e8986b184135e12839ab576c9a04203e85febfc5dcb35",
    ),
    (
        "macos",
        "aarch64",
        "aarch64-apple-darwin",
        "81d4a23d56b3c3eb8187658329116d50e0b228a93b343825fb71f70179051cd1",
    ),
];

const BINDGEN: &str = "wasm-bindgen";
const TEST_RUNNER: &str = "wasm-bindgen-test-runner";

/// Absolute paths of the two programs, at [`VERSION`].
pub(crate) struct Tools {
    pub(crate) bindgen: PathBuf,
    pub(crate) test_runner: PathBuf,
}

/// The version of the `wasm-bindgen` crate in a Cargo.lock.
pub(crate) fn locked_version(lock: &str) -> Result<String> {
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == "name = \"wasm-bindgen\"" {
            return lines
                .next()
                .and_then(|line| line.strip_prefix("version = \""))
                .and_then(|rest| rest.strip_suffix('"'))
                .map(str::to_string)
                .ok_or_else(|| "Cargo.lock: wasm-bindgen has no version line".into());
        }
    }
    Err("Cargo.lock has no wasm-bindgen crate".into())
}

/// Whether `program --version` says it is `name` at [`VERSION`].
fn is_pinned(program: &Path, name: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .ok()
        .filter(|result| result.status.success())
        .is_some_and(|result| {
            String::from_utf8_lossy(&result.stdout).trim() == format!("{name} {VERSION}")
        })
}

fn on_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .and_then(|path| fs::canonicalize(path).ok())
}

/// The pinned wasm-bindgen CLI: from PATH when it is the pinned version there, else fetched and checked.
pub(crate) fn tools() -> Result<Tools> {
    let locked = locked_version(&String::from_utf8_lossy(&read(&repo().join("Cargo.lock"))?))?;
    if locked != VERSION {
        return Err(format!(
            "Cargo.lock has the wasm-bindgen crate at {locked}, but xtask/src/wasm_bindgen.rs pins the CLI at \
             {VERSION}: pin the CLI of the same version (VERSION and the digests of its release assets)"
        ));
    }
    if let (Some(bindgen), Some(test_runner)) = (on_path(BINDGEN), on_path(TEST_RUNNER)) {
        if is_pinned(&bindgen, BINDGEN) && is_pinned(&test_runner, TEST_RUNNER) {
            return Ok(Tools {
                bindgen,
                test_runner,
            });
        }
    }

    let dir = target_dir()?
        .join("tools")
        .join(format!("wasm-bindgen-{VERSION}"));
    let tools = Tools {
        bindgen: dir.join(BINDGEN),
        test_runner: dir.join(TEST_RUNNER),
    };
    if is_pinned(&tools.bindgen, BINDGEN) && is_pinned(&tools.test_runner, TEST_RUNNER) {
        return Ok(tools);
    }

    let (os, arch) = (env::consts::OS, env::consts::ARCH);
    let (triple, digest) = BUILDS
        .iter()
        .find(|build| build.0 == os && build.1 == arch)
        .map(|build| (build.2, build.3))
        .ok_or_else(|| format!("no pinned wasm-bindgen CLI for {os}-{arch}"))?;
    let name = format!("wasm-bindgen-{VERSION}-{triple}");
    let url = format!("{RELEASES}/{VERSION}/{name}.tar.gz");
    eprintln!("xtask: fetching {url}");
    let bytes = download(&url)?;
    if sha256(&bytes) != digest {
        return Err(format!(
            "{url}: SHA-256 {}, but {digest} is pinned",
            sha256(&bytes)
        ));
    }
    let work = TempDir::new("sidevoice-engine-wasm-bindgen")?;
    let archive = work.0.join("wasm-bindgen.tar.gz");
    write(&archive, &bytes)?;
    output(
        "tar",
        &["-xzf", path_str(&archive)?, "-C", path_str(&work.0)?],
        None,
    )?;
    empty_dir(&dir)?;
    for program in [BINDGEN, TEST_RUNNER] {
        let target = dir.join(program);
        fs::copy(work.0.join(&name).join(program), &target)
            .map_err(|error| format!("{name}/{program}: {error}"))?;
        chmod_executable(&target)?;
        if !is_pinned(&target, program) {
            return Err(format!(
                "{}: does not run as {program} {VERSION}",
                target.display()
            ));
        }
    }
    Ok(tools)
}

fn chmod_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locked_version_is_the_wasm_bindgen_crate_s_own() {
        let lock = "[[package]]\nname = \"wasm-bindgen-futures\"\nversion = \"0.4.79\"\n\n\
                    [[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n";
        assert_eq!(locked_version(lock).unwrap(), "0.2.129");
        assert!(locked_version("[[package]]\nname = \"js-sys\"\n").is_err());
    }

    #[test]
    fn every_build_host_has_a_full_digest() {
        for (os, arch, triple, digest) in BUILDS {
            assert!(triple.contains(arch), "{os}-{arch}: {triple}");
            assert!(
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "{triple}"
            );
        }
    }
}
