/// macOS: Apple silicon where the build targets `aarch64`; an Intel Mac is not that class.
pub struct Os;

impl super::System for Os {
    const OS: &'static str = "macos";
    #[cfg(test)]
    const APPLE_SILICON: bool = cfg!(target_arch = "aarch64");
}
