//! The Origin Private File System (OPFS), the browser's private file system for a site, as the web build uses it:
//! directories, files, writing a file through a stream and moving it into place. Only what the engine needs is bound,
//! from `navigator.storage.getDirectory()` down; a failure is a JavaScript value for the caller to turn into its code.
//!
//! A *location* is a `/`-separated path from the OPFS root (`sidevoice-engine/blobs/<sha256>`,
//! `sidevoice-engine/models/<build id>/onnx/model_q8.onnx`): what `WebStorage` says a
//! stored file is at, and what the transformers.js backend opens a file by.

use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen]
extern "C" {
    /// `FileSystemDirectoryHandle`.
    #[derive(Clone)]
    pub(crate) type Directory;

    #[wasm_bindgen(method, catch, js_name = getDirectoryHandle)]
    async fn get_directory_handle(
        this: &Directory,
        name: &str,
        options: &JsValue,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = getFileHandle)]
    async fn get_file_handle(
        this: &Directory,
        name: &str,
        options: &JsValue,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = removeEntry)]
    async fn remove_entry(this: &Directory, name: &str) -> Result<JsValue, JsValue>;

    /// `FileSystemFileHandle`.
    #[derive(Clone)]
    pub(crate) type FileHandle;

    #[wasm_bindgen(method, catch, js_name = getFile)]
    async fn get_file(this: &FileHandle) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = createWritable)]
    async fn create_writable(this: &FileHandle) -> Result<JsValue, JsValue>;

    /// `FileSystemHandle.move(directory, name)`: renames the file, replacing one already there.
    #[wasm_bindgen(method, catch, js_name = "move")]
    async fn move_to(
        this: &FileHandle,
        directory: &Directory,
        name: &str,
    ) -> Result<JsValue, JsValue>;

    /// `FileSystemWritableFileStream`: what is written goes to a swap file, applied when it is closed.
    pub(crate) type Writable;

    #[wasm_bindgen(method, catch)]
    async fn write(this: &Writable, data: &Uint8Array) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = write)]
    async fn write_blob(this: &Writable, data: &web_sys::Blob) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch)]
    async fn close(this: &Writable) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch)]
    async fn abort(this: &Writable) -> Result<JsValue, JsValue>;
}

/// The OPFS root of this origin, if the page has one (`navigator.storage.getDirectory`; Node has none).
pub(crate) async fn root() -> Result<Directory, JsValue> {
    let navigator = Reflect::get(&js_sys::global(), &"navigator".into())?;
    let storage = Reflect::get(&navigator, &"storage".into())?;
    let get = Reflect::get(&storage, &"getDirectory".into())?;
    let get: js_sys::Function = get
        .dyn_into()
        .map_err(|_| JsValue::from_str("no OPFS here"))?;
    let promise: js_sys::Promise = get.call0(&storage)?.dyn_into()?;
    Ok(wasm_bindgen_futures::JsFuture::from(promise)
        .await?
        .unchecked_into())
}

/// Whether `error` is the `TypeMismatchError` an entry of the other kind (a file for a directory) fails with.
fn is_type_mismatch(error: &JsValue) -> bool {
    Reflect::get(error, &"name".into())
        .ok()
        .and_then(|name| name.as_string())
        .is_some_and(|name| name == "TypeMismatchError")
}

/// Whether `error` is the `NotFoundError` a missing entry fails with.
pub(crate) fn is_not_found(error: &JsValue) -> bool {
    Reflect::get(error, &"name".into())
        .ok()
        .and_then(|name| name.as_string())
        .is_some_and(|name| name == "NotFoundError")
}

/// `{ create }`, the options of `getFileHandle` and `getDirectoryHandle`.
fn create(create: bool) -> JsValue {
    let options = js_sys::Object::new();
    Reflect::set(&options, &"create".into(), &create.into()).expect("a plain object");
    options.into()
}

impl Directory {
    /// The directory `name` inside this one, created if it is not there.
    pub(crate) async fn directory(&self, name: &str) -> Result<Directory, JsValue> {
        let handle = self.get_directory_handle(name, &create(true)).await?;
        Ok(handle.unchecked_into())
    }

    /// The directory `name` inside this one, if it is there.
    pub(crate) async fn existing(&self, name: &str) -> Result<Option<Directory>, JsValue> {
        match self.get_directory_handle(name, &create(false)).await {
            Ok(handle) => Ok(Some(handle.unchecked_into())),
            // A file of that name is not a directory either.
            Err(error) if is_not_found(&error) || is_type_mismatch(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The directory at the `/`-separated `path` below this one: created where `create`, else `None` if it is not
    /// all there.
    pub(crate) async fn at(&self, path: &str, create: bool) -> Result<Option<Directory>, JsValue> {
        let mut directory = self.clone();
        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            directory = if create {
                directory.directory(segment).await?
            } else {
                match directory.existing(segment).await? {
                    Some(directory) => directory,
                    None => return Ok(None),
                }
            };
        }
        Ok(Some(directory))
    }

    /// The file `name` inside this one: `None` if there is none and `create` is false.
    pub(crate) async fn file(
        &self,
        name: &str,
        create: bool,
    ) -> Result<Option<FileHandle>, JsValue> {
        match self.get_file_handle(name, &self::create(create)).await {
            Ok(handle) => Ok(Some(handle.unchecked_into())),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Removes the entry `name`; nothing happens if there is none.
    pub(crate) async fn remove(&self, name: &str) -> Result<(), JsValue> {
        match self.remove_entry(name).await {
            Ok(_) => Ok(()),
            Err(error) if is_not_found(&error) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl FileHandle {
    /// What the file holds now, as a `File` (a `Blob`).
    pub(crate) async fn blob(&self) -> Result<web_sys::Blob, JsValue> {
        Ok(self.get_file().await?.unchecked_into())
    }

    /// A stream that writes the file anew, applied only when it is closed.
    pub(crate) async fn writable(&self) -> Result<Writable, JsValue> {
        Ok(self.create_writable().await?.unchecked_into())
    }

    /// Renames the file to `name` in `directory`, replacing a file already there.
    pub(crate) async fn rename(&self, directory: &Directory, name: &str) -> Result<(), JsValue> {
        self.move_to(directory, name).await.map(drop)
    }
}

impl Writable {
    /// Appends `bytes`.
    pub(crate) async fn append(&self, bytes: &[u8]) -> Result<(), JsValue> {
        self.write(&Uint8Array::from(bytes)).await.map(drop)
    }

    /// Appends what `blob` holds (a stored file, copied without passing through wasm memory).
    pub(crate) async fn append_blob(&self, blob: &web_sys::Blob) -> Result<(), JsValue> {
        self.write_blob(blob).await.map(drop)
    }

    /// Applies what was written to the file.
    pub(crate) async fn finish(&self) -> Result<(), JsValue> {
        self.close().await.map(drop)
    }

    /// Discards what was written.
    pub(crate) async fn discard(&self) -> Result<(), JsValue> {
        self.abort().await.map(drop)
    }
}

/// The file at `location`, a path from the OPFS root, as a `Blob`; `None` if it is not there.
pub(crate) async fn open(location: &str) -> Result<Option<web_sys::Blob>, JsValue> {
    let (parents, name) = location.rsplit_once('/').unwrap_or(("", location));
    let mut directory = root().await?;
    for segment in parents.split('/').filter(|segment| !segment.is_empty()) {
        directory = match directory
            .get_directory_handle(segment, &create(false))
            .await
        {
            Ok(handle) => handle.unchecked_into(),
            Err(error) if is_not_found(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
    }
    match directory.file(name, false).await? {
        Some(file) => file.blob().await.map(Some),
        None => Ok(None),
    }
}

impl Directory {
    /// Removes the entry `name` and everything in it; nothing happens if there is none.
    pub(crate) async fn remove_recursively(&self, name: &str) -> Result<(), JsValue> {
        let options = js_sys::Object::new();
        Reflect::set(&options, &"recursive".into(), &true.into())?;
        let remove: js_sys::Function = Reflect::get(self, &"removeEntry".into())?.dyn_into()?;
        let promise: js_sys::Promise = remove.call2(self, &name.into(), &options)?.dyn_into()?;
        match wasm_bindgen_futures::JsFuture::from(promise).await {
            Err(error) if !is_not_found(&error) => Err(error),
            _ => Ok(()),
        }
    }

    /// The names of the entries in it.
    pub(crate) async fn names(&self) -> Result<Vec<String>, JsValue> {
        let keys: js_sys::Function = Reflect::get(self, &"keys".into())?.dyn_into()?;
        let iterator = keys.call0(self)?;
        let next: js_sys::Function = Reflect::get(&iterator, &"next".into())?.dyn_into()?;
        let mut names = Vec::new();
        loop {
            let step: js_sys::Promise = next.call0(&iterator)?.dyn_into()?;
            let step = wasm_bindgen_futures::JsFuture::from(step).await?;
            if Reflect::get(&step, &"done".into())?.is_truthy() {
                return Ok(names);
            }
            names.extend(Reflect::get(&step, &"value".into())?.as_string());
        }
    }
}
