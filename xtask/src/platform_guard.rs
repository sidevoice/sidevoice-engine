//! A token check, not a Rust type checker: every cfg branch is inspected, on every host.
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::{read, repo, run_in, Result};

pub fn check() -> Result<()> {
    let root = repo();
    // Include untracked sources locally too; ignored build output is never scanned.
    let files = run_in(
        &root,
        "git ls-files --cached --others --exclude-standard -z",
        &[],
    )?;
    let mut errors = Vec::new();
    let mut total = 0;
    for path in files.split('\0').filter(|path| !path.is_empty()) {
        if exempt(Path::new(path)) || !path.ends_with(".rs") {
            continue;
        }
        let source =
            String::from_utf8(read(&root.join(path))?).map_err(|error| error.to_string())?;
        let tokens: TokenStream = source.parse().map_err(|error| format!("{path}: {error}"))?;
        let count = occurrences(tokens);
        if count > 0 {
            total += count;
            errors.push(format!(
                "{path}: {count} occurrences; move OS code into platform"
            ));
        }
    }
    if !errors.is_empty() {
        let files = errors.len();
        errors.push(format!(
            "platform boundary: {total} occurrences in {files} files"
        ));
        return Err(errors.join("\n"));
    }
    println!("platform boundary: no OS-specific code outside platform");
    Ok(())
}

fn exempt(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "platform.rs")
        || path.components().any(|part| part.as_os_str() == "platform")
}

fn ident(token: Option<&TokenTree>, name: &str) -> bool {
    matches!(token, Some(TokenTree::Ident(value)) if value == name)
}

fn colon(token: Option<&TokenTree>) -> bool {
    matches!(token, Some(TokenTree::Punct(value)) if value.as_char() == ':')
}

/// One occurrence per OS-sensitive cfg/cfg_attr/cfg! invocation or OS API path/identifier.
/// Tokenization ignores comments and literal contents, and handles multiline attributes.
fn occurrences(stream: TokenStream) -> usize {
    let tokens: Vec<_> = stream.into_iter().collect();
    let mut count = 0;
    let mut skip_until = 0;
    for (i, token) in tokens.iter().enumerate() {
        if i < skip_until {
            continue;
        }
        if ident(Some(token), "use") {
            if let Some(end) = tokens[i..]
                .iter()
                .position(|token| matches!(token, TokenTree::Punct(p) if p.as_char() == ';'))
            {
                let end = i + end + 1;
                let item: TokenStream = tokens[i..end].iter().cloned().collect();
                if let Ok(item) = syn::parse_str::<syn::ItemUse>(&item.to_string()) {
                    count += use_occurrences(&item.tree, false);
                    skip_until = end;
                    continue;
                }
            }
        }
        if let TokenTree::Group(group) = token {
            count += occurrences(group.stream());
        }
        if ident(Some(token), "cfg") || ident(Some(token), "cfg_attr") {
            let next = if matches!(tokens.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
            {
                i + 2
            } else {
                i + 1
            };
            if let Some(TokenTree::Group(group)) = tokens.get(next) {
                if group.delimiter() == Delimiter::Parenthesis && os_condition(group.stream()) {
                    count += 1;
                }
            }
        }
        let path = (colon(tokens.get(i + 1)) && colon(tokens.get(i + 2)))
            || (i >= 2 && colon(tokens.get(i - 1)) && colon(tokens.get(i - 2)));
        if (ident(Some(token), "std") && path && ident(tokens.get(i + 3), "os"))
            || (path
                && ["libc", "rustix", "windows_sys", "windows"]
                    .iter()
                    .any(|name| ident(Some(token), name)))
            || ["objc2", "core_foundation"]
                .iter()
                .any(|name| ident(Some(token), name))
        {
            count += 1;
        }
    }
    count
}

/// Keep the std prefix through grouped use trees; renames retain their original segment.
fn use_occurrences(tree: &syn::UseTree, in_std: bool) -> usize {
    let segment = |name: &syn::Ident| {
        usize::from(
            (in_std && name == "os")
                || [
                    "libc",
                    "rustix",
                    "windows_sys",
                    "windows",
                    "objc2",
                    "core_foundation",
                ]
                .iter()
                .any(|value| name == value),
        )
    };
    match tree {
        syn::UseTree::Path(path) => {
            segment(&path.ident) + use_occurrences(&path.tree, in_std || path.ident == "std")
        }
        syn::UseTree::Name(name) => segment(&name.ident),
        syn::UseTree::Rename(rename) => segment(&rename.ident),
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .map(|tree| use_occurrences(tree, in_std))
            .sum(),
        syn::UseTree::Glob(_) => 0,
    }
}

fn os_condition(stream: TokenStream) -> bool {
    let tokens: Vec<_> = stream.into_iter().collect();
    tokens.iter().enumerate().any(|(i, token)| {
        if let TokenTree::Group(group) = token {
            return os_condition(group.stream());
        }
        ["target_os", "unix", "windows"].iter().any(|name| ident(Some(token), name))
            || (ident(Some(token), "target_family")
                && !matches!(tokens.get(i + 2), Some(TokenTree::Literal(value)) if value.to_string() == "\"wasm\""))
    })
}

#[cfg(test)]
mod tests;
