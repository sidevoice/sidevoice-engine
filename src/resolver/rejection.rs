//! Why a build does not run here: the step of the funnel that rejected it, and the reason a person reads.

/// The step of the funnel that rejected a build.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Rejection {
    /// 1. The build's backend is not compiled into this build of the engine.
    BackendNotInThisBuild,
    /// 2. The backend cannot run here: none of its accelerators works here (`no-accelerator`), or it runs nothing yet
    ///    (`not-implemented`, a stub).
    BackendUnavailable(Reason),
    /// 3. The build does not fit here: none of the accelerators that work here is one its `requires` allows
    ///    (`build-accelerator`), in a page it needs more memory than WebAssembly hands it (`wasm-memory`, with the
    ///    build's memory and its `requires.wasm_max_mb`), or the machine does not meet a requirement of the build or of
    ///    its backend (`memory`, `cores`, ...).
    DoesNotFit(Reason),
}

/// Why a backend or a build does not fit: a stable code that clients translate (never text in a language), with the
/// numbers the message needs (what is needed, what there is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reason {
    /// The stable code, such as `"memory"`.
    pub code: &'static str,
    /// What is needed, when it is a number.
    pub needs: Option<u32>,
    /// What there is, when it is a number.
    pub has: Option<u32>,
}

impl Reason {
    /// A reason with no numbers.
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self {
            code,
            needs: None,
            has: None,
        }
    }

    /// A reason with what is needed and what there is.
    #[must_use]
    pub const fn with_numbers(code: &'static str, needs: u32, has: u32) -> Self {
        Self {
            code,
            needs: Some(needs),
            has: Some(has),
        }
    }
}
