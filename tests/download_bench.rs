//! How fast the host downloads one large file into its storage, as an install does (fetch, hash, write, commit).
//! Ignored unless asked for; the `download-bench` workflow asks. `DOWNLOAD_BENCH_URL` names the file.
//!
//! ```sh
//! DOWNLOAD_BENCH_URL=<url> cargo test --locked --test download_bench -- --ignored --nocapture
//! ```

#![cfg(native)]

use std::time::Instant;

use sha2::{Digest, Sha256};
use sidevoice_engine::{Download, Fetcher, Host, NativeHost, Storage, StorageWriter};

#[test]
#[ignore = "downloads a large file"]
fn download_into_storage() {
    let url = std::env::var("DOWNLOAD_BENCH_URL").expect("DOWNLOAD_BENCH_URL");
    let dir = tempfile_dir();
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let host = NativeHost::new(&dir).unwrap();
    runtime.block_on(async {
        let started = Instant::now();
        let mut download = host.fetcher().fetch(&url).await.unwrap();
        // Named as a blob is; the storage does not check the name against the bytes.
        let mut file = host.storage().create(&"0".repeat(64)).await.unwrap();
        let mut hasher = Sha256::new();
        let (mut bytes, mut parts) = (0u64, 0u64);
        let mut spent = [0f64; 3];
        loop {
            let at = Instant::now();
            let Some(part) = download.chunk().await.unwrap() else { break };
            spent[0] += at.elapsed().as_secs_f64();
            let at = Instant::now();
            hasher.update(&part);
            spent[1] += at.elapsed().as_secs_f64();
            let at = Instant::now();
            file.write(&part).await.unwrap();
            spent[2] += at.elapsed().as_secs_f64();
            bytes += part.len() as u64;
            parts += 1;
        }
        drop(hasher);
        let at = Instant::now();
        file.commit().await.unwrap();
        let commit = at.elapsed().as_secs_f64();
        let seconds = started.elapsed().as_secs_f64();
        println!(
            "BENCH {bytes} bytes in {seconds:.1} s = {:.1} MB/s ({parts} parts, {} B each)",
            bytes as f64 / 1e6 / seconds,
            bytes / parts.max(1)
        );
        println!("BENCH network {:.1} s, hash {:.1} s, write {:.1} s, commit {commit:.1} s", spent[0], spent[1], spent[2]);
    });
    drop(host);
    let _ = std::fs::remove_dir_all(dir);
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("download-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
