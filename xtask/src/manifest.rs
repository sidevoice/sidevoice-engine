//! `cargo xtask manifest DIR [--tag vX.Y.Z]`: the npm tarball's published name and `SHA256SUMS` for the assets in
//! DIR.

use std::env;
use std::fs;
use std::path::Path;

use crate::npm::{package_identity, tarball_name, PACKAGE};
use crate::util::*;
use crate::Result;

/// The name the npm tarball is published under on GitHub Releases: the one `npm pack` gives it for a release,
/// `sidevoice-engine-nightly.tgz` for the nightly (a fixed name, so its download URL never changes).
pub(crate) fn published_name(label: &str) -> String {
    format!("sidevoice-engine-{label}.tgz")
}

pub(crate) fn manifest(dir: &Path, tag: Option<&str>) -> Result<()> {
    let source_sha = git(&["rev-parse", "HEAD"])?;
    if let Ok(expected) = env::var("GITHUB_SHA") {
        // The attestation names GITHUB_SHA as its source: the assets must come from that very commit.
        if expected != source_sha {
            return Err(format!(
                "checked out {source_sha}, but this run is for {expected}"
            ));
        }
    }
    print!("{}", write_sums(dir, tag, &engine_version()?)?);
    Ok(())
}

/// Gives the npm tarball in `dir` its published name, checks it is `@sidevoice/engine` at `version`, and writes
/// `SHA256SUMS` over every asset in `dir`. Returns its text.
pub(crate) fn write_sums(dir: &Path, tag: Option<&str>, version: &str) -> Result<String> {
    if let Some(tag) = tag {
        if format!("v{version}") != tag {
            return Err(format!("Cargo.toml says {version}, the release is {tag}"));
        }
    }
    let name = published_name(tag.map_or("nightly", |tag| tag.trim_start_matches('v')));
    let built = dir.join(tarball_name(version));
    if built.exists() {
        fs::rename(built, dir.join(&name)).map_err(|error| format!("{name}: {error}"))?;
    }
    let identity = package_identity(&dir.join(&name))?;
    if identity.0 != PACKAGE || identity.1 != version {
        return Err(format!(
            "{name} is {}@{}, not {PACKAGE}@{version}",
            identity.0, identity.1
        ));
    }
    let mut sums = String::new();
    // An earlier SHA256SUMS is rewritten, not listed.
    for asset in files_below(dir)?
        .into_iter()
        .filter(|asset| asset != "SHA256SUMS")
    {
        if asset.contains('/') {
            return Err(format!(
                "{}: unexpected {asset} among the assets",
                dir.display()
            ));
        }
        sums.push_str(&format!("{}  {asset}\n", sha256(&read(&dir.join(&asset))?)));
    }
    write(&dir.join("SHA256SUMS"), sums.as_bytes())?;
    Ok(sums)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::npm::tests::tarball;

    #[test]
    fn the_nightly_name_is_fixed_and_a_release_s_is_npm_s() {
        assert_eq!(published_name("nightly"), "sidevoice-engine-nightly.tgz");
        assert_eq!(published_name("0.2.0-rc.1"), tarball_name("0.2.0-rc.1"));
    }

    #[test]
    fn a_release_lists_its_tarball() {
        let dir = TempDir::new("xtask-manifest-release").unwrap();
        tarball(&dir.0, PACKAGE, "0.2.0");
        let sums = write_sums(&dir.0, Some("v0.2.0"), "0.2.0").unwrap();
        let sums = parse_sums(sums.as_bytes()).unwrap();
        assert_eq!(sums.len(), 1);
        assert_eq!(sums[0].1, "sidevoice-engine-0.2.0.tgz");
        assert_eq!(
            sums[0].0,
            sha256(&read(&dir.0.join("sidevoice-engine-0.2.0.tgz")).unwrap())
        );
        assert_eq!(read(&dir.0.join("SHA256SUMS")).unwrap().len(), 66 + 26 + 1);
        // Run again: the same sums, SHA256SUMS not among them.
        let again = write_sums(&dir.0, Some("v0.2.0"), "0.2.0").unwrap();
        assert_eq!(parse_sums(again.as_bytes()).unwrap(), sums);
    }

    #[test]
    fn the_nightly_tarball_is_renamed() {
        let dir = TempDir::new("xtask-manifest-nightly").unwrap();
        tarball(&dir.0, PACKAGE, "0.2.0");
        let sums = write_sums(&dir.0, None, "0.2.0").unwrap();
        assert!(sums.ends_with("  sidevoice-engine-nightly.tgz\n"), "{sums}");
        assert!(!dir.0.join("sidevoice-engine-0.2.0.tgz").exists());
    }

    #[test]
    fn a_tag_other_than_the_crate_version_is_refused() {
        let dir = TempDir::new("xtask-manifest-tag").unwrap();
        tarball(&dir.0, PACKAGE, "0.2.0");
        let error = write_sums(&dir.0, Some("v0.3.0"), "0.2.0").unwrap_err();
        assert!(error.contains("0.3.0"), "{error}");
    }

    #[test]
    fn a_package_of_another_version_is_refused() {
        let dir = TempDir::new("xtask-manifest-version").unwrap();
        tarball(&dir.0, PACKAGE, "0.1.0");
        fs::rename(
            dir.0.join(tarball_name("0.1.0")),
            dir.0.join(tarball_name("0.2.0")),
        )
        .unwrap();
        let error = write_sums(&dir.0, Some("v0.2.0"), "0.2.0").unwrap_err();
        assert!(error.contains("@sidevoice/engine@0.1.0"), "{error}");
    }
}
