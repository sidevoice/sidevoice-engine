/// Linux: no Apple silicon.
pub struct Os;

impl super::System for Os {
    const OS: &'static str = "linux";
    #[cfg(test)]
    const APPLE_SILICON: bool = false;
}
