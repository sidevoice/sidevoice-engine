// The smallest program that uses the engine, built by `cargo xtask link-size`: an engine on the built-in host, over a
// directory it never writes to, with no catalogue, which prints the backends it was built with.

use sidevoice_engine::{Engine, NativeHost};

fn main() {
    let dir = std::env::temp_dir().join("sidevoice-link-size-probe");
    let host = NativeHost::new(dir).expect("a host");
    let engine = Engine::new(Box::new(host), Vec::new()).expect("an engine");
    let ids: Vec<_> = engine.backends().iter().map(|backend| backend.id).collect();
    println!("{}", ids.join(","));
}
