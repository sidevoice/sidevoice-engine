//! The sherpa-onnx config fields a build's files fill, generated from the source of the `sherpa-onnx` crate that
//! Cargo.lock pins, so that nobody types them: `src/backend/implementations/sherpa_onnx/config/fields.rs`.
//!
//! The crate's source is read with `syn` from where Cargo keeps it (`cargo metadata`). From the two roots the engine
//! fills, the offline recognizer's model config (`OfflineModelConfig`) and the offline TTS's (`OfflineTtsModelConfig`),
//! every field is followed into the crate's own structs, and every `Option<String>` met is a path, named as the crate
//! names it from its root (`whisper.encoder`, `tokens`, `kokoro.data_dir`), except the few that are options rather
//! than files ([`NOT_FILES`]): the crate types both alike. The file is written through `rustfmt`; `--check` makes it
//! again and fails if the committed one differs, or if [`NOT_FILES`] names a field the crate no longer has.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{empty_dir, read, repo, run_in, sh, write, Result};

/// Where the generated table is committed, relative to the repository.
pub(crate) const FIELDS: &str = "src/backend/implementations/sherpa_onnx/config/fields.rs";

/// The `Option<String>` fields that are not files, by path from their root: languages, tasks, prompts and the like,
/// set per call or as options (sidevoice-engine#45, #46), never from a build's files. The crate types them as it types
/// paths, so this is the one list a person keeps.
const NOT_FILES: &[(&str, &str)] = &[
    ("OfflineModelConfig", "provider"),
    ("OfflineModelConfig", "model_type"),
    ("OfflineModelConfig", "modeling_unit"),
    ("OfflineModelConfig", "whisper.language"),
    ("OfflineModelConfig", "whisper.task"),
    ("OfflineModelConfig", "canary.src_lang"),
    ("OfflineModelConfig", "canary.tgt_lang"),
    ("OfflineModelConfig", "sense_voice.language"),
    ("OfflineModelConfig", "cohere_transcribe.language"),
    ("OfflineModelConfig", "qwen3_asr.hotwords"),
    ("OfflineModelConfig", "funasr_nano.hotwords"),
    ("OfflineModelConfig", "funasr_nano.language"),
    ("OfflineModelConfig", "funasr_nano.system_prompt"),
    ("OfflineModelConfig", "funasr_nano.user_prompt"),
    ("OfflineTtsModelConfig", "provider"),
    ("OfflineTtsModelConfig", "kokoro.lang"),
];

/// The fields of [`NOT_FILES`] below `OfflineModelConfig` that a call's argument may set, through a build's `call_params`
/// (sidevoice-engine#46): languages and tasks. The generated `stt_option` names exactly these.
const CALL_FIELDS: &[&str] = &[
    "whisper.language",
    "whisper.task",
    "canary.src_lang",
    "canary.tgt_lang",
    "sense_voice.language",
    "cohere_transcribe.language",
    "funasr_nano.language",
];

/// A struct of the crate: its named fields and their types.
type Structs = HashMap<String, Vec<(String, syn::Type)>>;

/// The generated file, as it should be committed, and the crate's version.
pub(crate) fn generate() -> Result<(String, String)> {
    let (version, src) = crate_source()?;
    let structs = parse(&src)?;
    let stt = paths(&structs, "OfflineModelConfig")?;
    let tts = paths(&structs, "OfflineTtsModelConfig")?;
    let options = call_fields()?;
    let text = render(&version, &stt, &tts, &options);
    Ok((rustfmt(&text)?, version))
}

/// The pinned `sherpa-onnx` crate's version and its `src/` directory, from `cargo metadata`.
fn crate_source() -> Result<(String, PathBuf)> {
    let meta = sh("cargo metadata --locked --format-version 1")?;
    let meta: serde_json::Value = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
    let package = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| package["name"] == "sherpa-onnx")
        .ok_or("cargo metadata: no sherpa-onnx package")?;
    let version = package["version"]
        .as_str()
        .ok_or("sherpa-onnx: no version")?;
    let manifest = package["manifest_path"]
        .as_str()
        .ok_or("sherpa-onnx: no manifest path")?;
    let src = Path::new(manifest)
        .parent()
        .ok_or("sherpa-onnx: manifest without a directory")?
        .join("src");
    Ok((version.to_owned(), src))
}

/// Every struct with named fields in the `.rs` files of `src`, by name.
fn parse(src: &Path) -> Result<Structs> {
    let mut structs = Structs::new();
    let entries = std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text =
            String::from_utf8(read(&path)?).map_err(|e| format!("{}: {e}", path.display()))?;
        let file = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        for item in file.items {
            if let syn::Item::Struct(item) = item {
                if let syn::Fields::Named(fields) = item.fields {
                    let fields = fields
                        .named
                        .into_iter()
                        .filter_map(|field| Some((field.ident?.to_string(), field.ty)))
                        .collect();
                    structs.insert(item.ident.to_string(), fields);
                }
            }
        }
    }
    Ok(structs)
}

/// [`CALL_FIELDS`], each checked to be one of [`NOT_FILES`] below `OfflineModelConfig` (which [`paths`] checks the crate
/// has).
fn call_fields() -> Result<Vec<String>> {
    let options: Vec<&str> = NOT_FILES
        .iter()
        .filter(|(of, _)| *of == "OfflineModelConfig")
        .map(|(_, path)| *path)
        .collect();
    CALL_FIELDS
        .iter()
        .map(|path| {
            if options.contains(path) {
                Ok((*path).to_owned())
            } else {
                Err(format!("CALL_FIELDS names `{path}`, which NOT_FILES does not (xtask/src/sherpa_fields.rs)"))
            }
        })
        .collect()
}

/// Every file path below the struct `root`, in the crate's field order, and every name in [`NOT_FILES`] for it
/// checked to exist.
fn paths(structs: &Structs, root: &str) -> Result<Vec<String>> {
    let mut strings = Vec::new();
    walk(structs, root, "", &mut strings)?;
    let options: Vec<&str> = NOT_FILES
        .iter()
        .filter(|(of, _)| *of == root)
        .map(|(_, path)| *path)
        .collect();
    if let Some(gone) = options
        .iter()
        .find(|path| !strings.iter().any(|s| s == *path))
    {
        return Err(format!(
            "sherpa-onnx's {root} has no `{gone}` any more: drop it from NOT_FILES (xtask/src/sherpa_fields.rs)"
        ));
    }
    Ok(strings
        .into_iter()
        .filter(|path| !options.contains(&path.as_str()))
        .collect())
}

/// Collects into `out` every `Option<String>` field below `name`, as `prefix` + its path.
fn walk(structs: &Structs, name: &str, prefix: &str, out: &mut Vec<String>) -> Result<()> {
    let fields = structs
        .get(name)
        .ok_or(format!("sherpa-onnx: no struct {name}"))?;
    for (field, ty) in fields {
        let path = format!("{prefix}{field}");
        if is_option_string(ty) {
            out.push(path);
        } else if let Some(inner) = last_ident(ty).filter(|ident| structs.contains_key(ident)) {
            walk(structs, &inner, &format!("{path}."), out)?;
        }
    }
    Ok(())
}

/// The last segment of a plain type path, without generics: `OfflineWhisperModelConfig`.
fn last_ident(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    segment
        .arguments
        .is_none()
        .then(|| segment.ident.to_string())
}

/// Whether `ty` is `Option<String>`.
fn is_option_string(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    segment.ident == "Option"
        && matches!(args.args.first(), Some(syn::GenericArgument::Type(inner)) if last_ident(inner).as_deref() == Some("String"))
}

/// The file's text, before `rustfmt`.
fn render(version: &str, stt: &[String], tts: &[String], options: &[String]) -> String {
    let arms = |paths: &[String]| {
        paths
            .iter()
            .map(|path| format!("        \"{path}\" => &mut config.{path},\n"))
            .collect::<String>()
    };
    format!(
        "//! Generated by `cargo xtask sherpa-libs --pin` from the config structs of sherpa-onnx {version}, the crate \
         Cargo.lock\n\
         //! pins: do not edit. `cargo xtask sherpa-libs --check` fails when it is not what that version makes.\n\
         //!\n\
         //! Every field that takes a file, by its path from the config root it is in: the offline recognizer's model \
         config\n\
         //! (`OfflineRecognizerConfig.model_config`) for speech to text, the offline TTS's (`OfflineTtsConfig.model`) \
         for text\n\
         //! to speech. What a sherpa-onnx build's file keys name. If the crate derives serde for its configs one day, \
         this\n\
         //! table gives way to `serde_json::from_value`.\n\
         //!\n\
         //! And every field a call's argument may set through a build's `call_params`: a language, a task.\n\
         \n\
         use sherpa_onnx::{{OfflineModelConfig, OfflineTtsModelConfig}};\n\
         \n\
         /// The field of `OfflineRecognizerConfig.model_config` that `key` names, if it takes a file.\n\
         pub(in crate::backend::implementations::sherpa_onnx) fn stt_field<'a>(\n\
             config: &'a mut OfflineModelConfig,\n\
             key: &str,\n\
         ) -> Option<&'a mut Option<String>> {{\n\
             Some(match key {{\n{}        _ => return None,\n    }})\n}}\n\
         \n\
         /// The field of `OfflineTtsConfig.model` that `key` names, if it takes a file.\n\
         pub(in crate::backend::implementations::sherpa_onnx) fn tts_field<'a>(\n\
             config: &'a mut OfflineTtsModelConfig,\n\
             key: &str,\n\
         ) -> Option<&'a mut Option<String>> {{\n\
             Some(match key {{\n{}        _ => return None,\n    }})\n}}\n\
         \n\
         /// The field of `OfflineRecognizerConfig.model_config` that `path` names, if a call's argument may set it.\n\
         pub(in crate::backend::implementations::sherpa_onnx) fn stt_option<'a>(\n\
             config: &'a mut OfflineModelConfig,\n\
             path: &str,\n\
         ) -> Option<&'a mut Option<String>> {{\n\
             Some(match path {{\n{}        _ => return None,\n    }})\n}}\n",
        arms(stt),
        arms(tts),
        arms(options),
    )
}

/// `text` as `rustfmt` writes it, so that the committed file passes `cargo fmt --check`.
fn rustfmt(text: &str) -> Result<String> {
    let dir = std::env::temp_dir().join("xtask-sherpa-fields");
    empty_dir(&dir)?;
    let file = dir.join("fields.rs");
    write(&file, text.as_bytes())?;
    let path = file.to_string_lossy().into_owned();
    run_in(&repo(), "rustfmt --edition 2021", &[&path])?;
    String::from_utf8(read(&file)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
