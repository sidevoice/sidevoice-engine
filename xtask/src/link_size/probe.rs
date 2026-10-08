// The smallest program that uses the engine, built by `cargo xtask link-size` with and without the sherpa-onnx
// feature: an engine on a host that has nothing, which prints the backends it was built with.

use sidevoice_engine::{
    async_trait, Accelerator, Capabilities, Engine, Fetcher, Host, Result, Runs, Storage,
};

struct Nothing;

impl Host for Nothing {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            runs: Runs::Native,
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            accelerators: vec![Accelerator::Cpu],
            memory_mb: None,
            cores: None,
        }
    }

    fn storage(&self) -> &dyn Storage {
        self
    }

    fn fetcher(&self) -> &dyn Fetcher {
        self
    }
}

#[async_trait]
impl Storage for Nothing {
    async fn contains(&self, _key: &str) -> Result<bool> {
        Ok(false)
    }
}

#[async_trait]
impl Fetcher for Nothing {
    async fn fetch(&self, _url: &str, _sha256: &str, _key: &str) -> Result<()> {
        Ok(())
    }
}

fn main() {
    let engine = Engine::new(Box::new(Nothing), Vec::new()).expect("an engine");
    println!("{}", engine.backends().join(","));
}
