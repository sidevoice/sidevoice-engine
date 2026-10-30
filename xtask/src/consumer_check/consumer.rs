// A downstream app, as small as it gets, built by `cargo xtask consumer-check`: an engine on the built-in host, which
// prints the backends it was built with. Running it proves the static libraries load (a mixed C runtime fails at link
// time, a missing one when it starts).

use sidevoice_engine::{BundledCatalog, Engine, NativeHost};

fn main() {
    let dir = std::env::temp_dir().join("sidevoice-consumer-check-host");
    let host = NativeHost::new(dir).expect("a host");
    let engine = Engine::new(Box::new(host), vec![Box::new(BundledCatalog)]).expect("an engine");
    let ids: Vec<_> = engine.backends().iter().map(|backend| backend.id).collect();
    println!("{}", ids.join(","));
}
