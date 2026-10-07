/// Whether the engine runs in a native process or in a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Runs {
    /// A native process: desktop, headless, a server.
    Native,
    /// A web page (the wasm32 build).
    Page,
}
