//! The native host's downloads, over HTTP(S). The client (`ureq`) is blocking, so each download runs on a thread of
//! its own and hands its parts to the async side through a bounded channel: the engine's futures need no particular
//! executor, and a slow disk or consumer slows the download instead of filling memory. Dropping the download closes
//! the channel, and the thread stops at its next part.

use std::fmt;
use std::io::{ErrorKind, Read};
use std::thread;
use std::time::Duration;

use async_channel::{Receiver, Sender};

use crate::{async_trait, Download, Error, Fetcher, Result};

/// The size of the parts a download is handed over in.
const PART: usize = 64 * 1024;
/// How many parts may wait to be taken before the download pauses.
const PARTS_AHEAD: usize = 4;

/// Downloads over HTTP(S), following redirects; an HTTP error status fails.
pub(super) struct Http {
    agent: ureq::Agent,
}

impl fmt::Debug for Http {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http").finish_non_exhaustive()
    }
}

impl Default for Http {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .build()
            .into();
        Self { agent }
    }
}

#[async_trait]
impl Fetcher for Http {
    async fn fetch(&self, url: &str) -> Result<Box<dyn Download>> {
        let (head, size) = async_channel::bounded(1);
        let (sender, parts) = async_channel::bounded(PARTS_AHEAD);
        let agent = self.agent.clone();
        let url = url.to_owned();
        thread::Builder::new()
            .name("sidevoice-download".to_owned())
            .spawn(move || download(&agent, &url, &head, &sender))
            .map_err(|_| failed())?;
        let size = size.recv().await.map_err(|_| failed())??;
        Ok(Box::new(HttpDownload { size, parts }))
    }
}

/// On its own thread: `url`'s size to `head` once the server answers, then its bytes to `parts`, a part at a time,
/// until they end, fail, or nobody is listening any more.
fn download(
    agent: &ureq::Agent,
    url: &str,
    head: &Sender<Result<Option<u64>>>,
    parts: &Sender<Result<Vec<u8>>>,
) {
    let body = match agent.get(url).call() {
        Ok(response) => response.into_body(),
        Err(_) => {
            let _ = head.send_blocking(Err(failed()));
            return;
        }
    };
    if head.send_blocking(Ok(body.content_length())).is_err() {
        return;
    }
    let mut reader = body.into_reader();
    loop {
        let mut part = vec![0; PART];
        let sent = match reader.read(&mut part) {
            Ok(0) => return,
            Ok(read) => {
                part.truncate(read);
                parts.send_blocking(Ok(part))
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => {
                let _ = parts.send_blocking(Err(failed()));
                return;
            }
        };
        if sent.is_err() {
            return;
        }
    }
}

/// A download under way: its parts arrive from its thread.
struct HttpDownload {
    size: Option<u64>,
    parts: Receiver<Result<Vec<u8>>>,
}

#[async_trait]
impl Download for HttpDownload {
    fn size(&self) -> Option<u64> {
        self.size
    }

    async fn chunk(&mut self) -> Result<Option<Vec<u8>>> {
        // The thread closes the channel once every part is sent.
        match self.parts.recv().await {
            Ok(part) => part.map(Some),
            Err(_closed) => Ok(None),
        }
    }
}

fn failed() -> Error {
    Error::new("download-failed")
}
