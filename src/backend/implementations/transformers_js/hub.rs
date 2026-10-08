//! transformers.js's hub, pointed at the engine's files. transformers.js reads a model's files by their path in its
//! repository, through its `env`: first a cache (`env.customCache`), then `env.fetch`. While the library is open, the
//! hub answers both for the models the engine serves, each under a name of its own (`sidevoice-engine-opfs/m<n>`):
//! the cache with the file in the build's folder, at its path in the repository (a `Response` over the OPFS file), `fetch` with a 404 for a path the build
//! does not have (an optional file such as `generation_config.json` of a model without one). Anything else goes where
//! it went before: the page's own use of transformers.js keeps its cache and its downloads. Closing the library puts
//! `env` back as it was.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use js_sys::{Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::catalog::BuildEntry;
use crate::install::Installed;
use crate::web::opfs;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// The name every served model is under, as a Hugging Face repository id: an owner of no repository the engine
/// would ever download from, since its requests never leave the page.
const OWNER: &str = "sidevoice-engine-opfs";

#[wasm_bindgen(inline_js = r#"
export function serve(env, owner, open) {
  const before = { fetch: env.fetch, useCustomCache: env.useCustomCache, customCache: env.customCache };
  // The path after `<owner>/`: `m3/onnx/model.onnx` (a local path) or `m3/resolve/main/onnx/model.onnx` (a URL).
  const ours = (key) => {
    const text = String(key instanceof Request ? key.url : key);
    const at = text.indexOf(owner + '/');
    return at < 0 ? null : text.slice(at + owner.length + 1);
  };
  // Where a file that is not ours is cached: where transformers.js would have cached it.
  const other = async () => {
    if (before.useCustomCache) return before.customCache;
    if (env.useBrowserCache && typeof caches !== 'undefined') {
      try { return await caches.open(env.cacheKey); } catch { return null; }
    }
    return null;
  };
  env.customCache = {
    async match(key) {
      const path = ours(key);
      if (path === null) return (await other())?.match(key);
      const blob = await open(path);
      if (!blob) return undefined;
      return new Response(blob, { headers: { 'content-length': String(blob.size) } });
    },
    async put(key, response) {
      if (ours(key) === null) return (await other())?.put(key, response);
    },
  };
  env.useCustomCache = true;
  env.fetch = (url, init) => ours(url) === null ? before.fetch(url, init) : Promise.resolve(new Response(null, { status: 404 }));
  return () => Object.assign(env, before);
}
"#)]
extern "C" {
    /// Points `env` at `open` for every path under `owner/`, and returns what puts it back.
    #[wasm_bindgen(catch)]
    fn serve(
        env: &JsValue,
        owner: &str,
        open: &Closure<dyn FnMut(String) -> Promise>,
    ) -> Result<js_sys::Function, JsValue>;
}

/// The hub while the library is open: which files each served model has, and what puts `env` back.
pub(super) struct Hub {
    models: Rc<RefCell<Models>>,
    restore: js_sys::Function,
    /// Called by transformers.js for as long as `env` points at it.
    _open: Closure<dyn FnMut(String) -> Promise>,
}

#[derive(Default)]
struct Models {
    /// By served name (`m3`): each path in the repository, and where the host stored it.
    files: BTreeMap<String, BTreeMap<String, String>>,
    next: u64,
}

impl Hub {
    /// Points the `env` of the transformers.js `module` at the models it will serve.
    pub(super) fn open(module: &JsValue) -> Result<Rc<Self>, JsValue> {
        let env = Reflect::get(module, &"env".into())?;
        let models = Rc::new(RefCell::new(Models::default()));
        let served = Rc::clone(&models);
        let open = Closure::new(move |path: String| {
            let location = served.borrow().location(&path);
            future_to_promise(async move {
                let Some(location) = location else {
                    return Ok(JsValue::NULL);
                };
                Ok(opfs::open(&location)
                    .await?
                    .map_or(JsValue::NULL, JsValue::from))
            })
        });
        let restore = serve(&env, OWNER, &open)?;
        Ok(Rc::new(Self {
            models,
            restore,
            _open: open,
        }))
    }

    /// Serves `build`'s `files` under a name of its own, until what is returned is dropped.
    pub(super) fn serve(hub: &Rc<Self>, build: &BuildEntry, files: &Installed) -> Result<Served> {
        let mut paths = BTreeMap::new();
        for file in &build.files {
            let path = repository_path(&file.url).ok_or(Error::new("unsupported-model"))?;
            let location = files
                .file(&file.key)
                .ok_or(Error::new("file-not-installed"))?;
            paths.insert(path.to_owned(), location.to_owned());
        }
        let mut models = hub.models.borrow_mut();
        models.next += 1;
        let name = format!("m{}", models.next);
        models.files.insert(name.clone(), paths);
        Ok(Served {
            hub: Rc::clone(hub),
            name,
        })
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        if let Err(error) = self.restore.call0(&JsValue::UNDEFINED) {
            web_sys::console::warn_2(
                &"sidevoice-engine: restoring transformers.js's env failed:".into(),
                &error,
            );
        }
    }
}

impl Models {
    /// Where the file at `path` (`m3/onnx/model.onnx`, or `m3/resolve/<revision>/onnx/model.onnx`) is stored.
    fn location(&self, path: &str) -> Option<String> {
        let (name, path) = path.split_once('/')?;
        let path = match path.strip_prefix("resolve/") {
            Some(rest) => rest.split_once('/')?.1,
            None => path,
        };
        self.files.get(name)?.get(path).cloned()
    }
}

/// A model's files, served while it lives.
pub(super) struct Served {
    hub: Rc<Hub>,
    name: String,
}

impl Served {
    /// The id transformers.js loads the model by: `sidevoice-engine-opfs/m<n>`.
    pub(super) fn id(&self) -> String {
        format!("{OWNER}/{}", self.name)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.hub.models.borrow_mut().files.remove(&self.name);
    }
}

/// The path of a file in its Hugging Face repository, from its pinned URL: what follows `/resolve/<revision>/`.
fn repository_path(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("/resolve/")?;
    let (_, path) = rest.split_once('/')?;
    (!path.is_empty()).then_some(path)
}
