/// The page: no operating system the engine can see, so it reports `web`, and has no Apple silicon class.
pub struct Os;

impl super::System for Os {
    const OS: &'static str = "web";
    #[cfg(test)]
    const APPLE_SILICON: bool = false;
}
