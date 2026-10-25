//! A token check, not a Rust type checker: every cfg branch is inspected, on every host.
use std::collections::BTreeMap;
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::{read, repo, run_in, Result};

pub fn check() -> Result<()> {
    let root = repo();
    let text = String::from_utf8(read(&root.join("platform-debt.txt"))?)
        .map_err(|error| error.to_string())?;
    let debt = parse_debt(&text)?;
    let mut errors = check_base(&root, &debt)?;
    // Include untracked sources locally too; ignored build output is never scanned.
    let files = run_in(
        &root,
        "git ls-files --cached --others --exclude-standard -z",
        &[],
    )?;
    let mut actual = BTreeMap::new();
    for path in files.split('\0').filter(|path| !path.is_empty()) {
        if exempt(Path::new(path)) || !path.ends_with(".rs") {
            continue;
        }
        let source =
            String::from_utf8(read(&root.join(path))?).map_err(|error| error.to_string())?;
        let tokens: TokenStream = source.parse().map_err(|error| format!("{path}: {error}"))?;
        let count = occurrences(tokens);
        if count > 0 {
            actual.insert(path.to_owned(), count);
        }
    }
    errors.extend(compare(&actual, &debt));
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    println!(
        "platform boundary: {} occurrences in {} debt files",
        actual.values().sum::<usize>(),
        actual.len()
    );
    Ok(())
}

fn parse_debt(text: &str) -> Result<BTreeMap<String, usize>> {
    let mut debt = BTreeMap::new();
    for line in text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
    {
        let (path, count) = line
            .split_once(' ')
            .ok_or("invalid platform-debt.txt entry")?;
        let count = count.parse().map_err(|_| "invalid platform debt count")?;
        if debt.insert(path.to_owned(), count).is_some() {
            return Err(format!("duplicate platform debt: {path}"));
        }
    }
    Ok(debt)
}

fn check_base(root: &Path, debt: &BTreeMap<String, usize>) -> Result<Vec<String>> {
    let branch = std::env::var("GITHUB_BASE_REF")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "main".into());
    let base = format!("refs/remotes/origin/{branch}");
    if run_in(
        root,
        "git rev-parse --verify",
        &[&format!("{base}^{{commit}}")],
    )
    .is_err()
    {
        if std::env::var_os("CI").is_some() {
            return Err(format!(
                "platform debt: missing {base}; fetch the base branch before checking"
            ));
        }
        println!("note: no {base}; skipping base-branch debt comparison locally");
        return Ok(Vec::new());
    }
    let files = run_in(root, "git ls-tree -r --name-only -z", &[&base])?;
    let base_debt = if files.split('\0').any(|path| path == "platform-debt.txt") {
        parse_debt(&run_in(
            root,
            "git show",
            &[&format!("{base}:platform-debt.txt")],
        )?)?
    } else {
        // Bootstrap this first guard PR from the base's source, never from the PR's allowance.
        let mut baseline = BTreeMap::new();
        for path in files
            .split('\0')
            .filter(|path| path.ends_with(".rs") && !exempt(Path::new(path)))
        {
            let source = run_in(root, "git show", &[&format!("{base}:{path}")])?;
            let tokens = source
                .parse()
                .map_err(|error| format!("{base}:{path}: {error}"))?;
            let count = occurrences(tokens);
            if count > 0 {
                baseline.insert(path.to_owned(), count);
            }
        }
        baseline
    };
    Ok(compare_debt(debt, &base_debt))
}

fn compare_debt(debt: &BTreeMap<String, usize>, base: &BTreeMap<String, usize>) -> Vec<String> {
    debt.iter()
        .filter_map(|(path, count)| match base.get(path) {
            Some(cap) if count <= cap => None,
            _ => Some(format!(
                "{path}: debt allowance {count} is new or increased against the base branch"
            )),
        })
        .collect()
}

fn exempt(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "platform.rs")
        || path.components().any(|part| part.as_os_str() == "platform")
}

fn compare(actual: &BTreeMap<String, usize>, debt: &BTreeMap<String, usize>) -> Vec<String> {
    let mut errors = Vec::new();
    for (path, count) in actual {
        let cap = debt.get(path).copied().unwrap_or(0);
        if *count > cap {
            errors.push(format!(
                "{path}: {count} occurrences (debt limit {cap}); move OS code into platform"
            ));
        }
    }
    for (path, cap) in debt {
        let count = actual.get(path).copied().unwrap_or(0);
        if count < *cap {
            errors.push(format!("lower platform-debt.txt entry for {path} from {cap} to {count} (remove it at zero); unused allowance cannot be retained"));
        }
    }
    errors
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
