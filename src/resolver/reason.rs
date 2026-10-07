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
