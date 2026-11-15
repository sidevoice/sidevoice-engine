/// iOS: not a Mac, so not the Apple silicon class.
pub struct Os;

impl super::System for Os {
    const OS: &'static str = "ios";
    #[cfg(test)]
    const APPLE_SILICON: bool = false;
}
