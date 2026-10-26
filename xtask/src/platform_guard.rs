//! Platform boundary check, including code inactive on this CI host.
//!
//! Reads Rust tokens, not text: comments and string contents are never code, so OS examples in docs and fixtures
//! cost nothing. What counts as an OS operation outside `platform` (the engine's own `platform` module):
//! - a path rooted in an OS crate (`libc`, `rustix`, `windows*`, `objc2*`, `core_foundation*`, `cocoa`…), `std::os`,
//!   or `std::env::consts::{OS, FAMILY}`;
//! - an OS condition (`target_os`, `target_family`, `target_vendor`, `unix`, `windows`) in `cfg`, `cfg_attr` or
//!   `cfg!`, or a `cfg_aliases!` definition of one (`build.rs` defines `apple_silicon`), and every use of such an
//!   alias. The web is not an OS: `target_arch = "wasm32"` and `target_family = "wasm"` are allowed everywhere;
//! - any name bound to an OS path by `use`/`extern crate` (`use libc as sys;` makes `sys::…` OS code);
//! - a `Command::new("<program>")` of a program that exists on one OS only (shells, `uname`, `launchctl`…);
//! - the Cargo OS variables (`CARGO_CFG_TARGET_OS`…) read by a build script.
//!
//! Limits, by design of a token scan: a program name or OS variable reaching `Command::new` through a constant
//! or a computed value, and a bare function imported by glob (`use libc::*`), are not followed.
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::Path,
    str::FromStr,
};

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::{repo, Result};

/// Crates (and prefixes of crate families) that are an OS API.
const OS_CRATES: [&str; 11] = [
    "libc",
    "rustix",
    "nix",
    "winapi",
    "winreg",
    "windows",
    "windows_sys",
    "windows_core",
    "cocoa",
    "objc",
    "core_graphics",
];
const OS_CRATE_PREFIXES: [&str; 3] = ["objc2", "core_foundation", "windows_"];
/// Names that make a `cfg`/`cfg_attr` predicate or an attribute OS-specific.
const OS_CONDITIONS: [&str; 6] = [
    "target_os",
    "target_family",
    "target_vendor",
    "unix",
    "windows",
    "windows_subsystem",
];
/// Cargo's view of the target OS, as build scripts read it: `CARGO_CFG_<suffix>`.
const OS_ENV_SUFFIXES: [&str; 4] = ["TARGET_OS", "TARGET_FAMILY", "UNIX", "WINDOWS"];
/// Programs that exist on one OS only (or whose behaviour is that OS's), matched on the file stem, lowercased.
const OS_PROGRAMS: [&str; 45] = [
    "sh",
    "bash",
    "zsh",
    "dash",
    "cmd",
    "powershell",
    "pwsh",
    "uname",
    "ps",
    "kill",
    "pkill",
    "killall",
    "taskkill",
    "tasklist",
    "wmic",
    "reg",
    "sc",
    "net",
    "icacls",
    "attrib",
    "chmod",
    "chown",
    "chgrp",
    "id",
    "whoami",
    "hostname",
    "lsof",
    "launchctl",
    "osascript",
    "open",
    "xdg-open",
    "defaults",
    "security",
    "sw_vers",
    "sysctl",
    "systemctl",
    "codesign",
    "hdiutil",
    "xcrun",
    "notarytool",
    "ditto",
    "spctl",
    "plutil",
    "signtool",
    "msiexec",
];

/// Functions that run a command line given as one string (`xtask`'s `sh` and `run_in`).
const COMMAND_HELPERS: [&str; 2] = ["sh", "run_in"];

/// Whether a program (a name or a path, with or without `.exe`/`.cmd`) exists on one OS only.
fn os_program(program: &str) -> bool {
    let file = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let stem = file
        .strip_suffix(".exe")
        .or_else(|| file.strip_suffix(".cmd"))
        .unwrap_or(&file);
    OS_PROGRAMS.contains(&stem)
}

fn is_os_crate(name: &str) -> bool {
    OS_CRATES.contains(&name)
        || OS_CRATE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn ident(tree: &TokenTree) -> Option<String> {
    match tree {
        TokenTree::Ident(id) => Some(id.to_string().trim_start_matches("r#").to_string()),
        _ => None,
    }
}

fn punct(tree: Option<&TokenTree>, ch: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(p)) if p.as_char() == ch)
}

fn group(tree: Option<&TokenTree>, delimiter: Delimiter) -> Option<TokenStream> {
    match tree {
        Some(TokenTree::Group(g)) if g.delimiter() == delimiter => Some(g.stream()),
        _ => None,
    }
}

fn flat(stream: TokenStream) -> Vec<TokenTree> {
    stream.into_iter().collect()
}

/// `::` at `at` (two joint colons).
fn path_sep(trees: &[TokenTree], at: usize) -> bool {
    punct(trees.get(at), ':') && punct(trees.get(at + 1), ':')
}

/// Whether the identifier at `at` continues a path (`a::HERE`) instead of starting one.
fn mid_path(trees: &[TokenTree], at: usize) -> bool {
    at >= 3 && path_sep(trees, at - 2) && ident(&trees[at - 3]).is_some()
}

/// OS conditions anywhere in a predicate (idents only: `feature = "windows"` is a string).
fn names_os(stream: &TokenStream, aliases: &HashSet<String>) -> bool {
    let trees = flat(stream.clone());
    trees.iter().enumerate().any(|(i, tree)| match tree {
        TokenTree::Ident(id) => {
            let name = id.to_string();
            // `target_family = "wasm"` names the web, which is not an OS.
            let wasm = name == "target_family"
                && matches!(trees.get(i + 2), Some(TokenTree::Literal(lit))
                    if string_value(lit).as_deref() == Some("wasm"));
            (OS_CONDITIONS.contains(&name.as_str()) && !wasm) || aliases.contains(&name)
        }
        TokenTree::Group(g) => names_os(&g.stream(), aliases),
        _ => false,
    })
}

/// Whether one attribute (`name` and the tokens of its parentheses) is an OS condition: a `cfg` predicate, a
/// `cfg_attr` predicate, or a `cfg`/`cfg_attr`/`windows_subsystem` among the attributes a `cfg_attr` applies.
/// Other payloads (`doc = stringify!(unix)`, `derive(..)`) are data, not conditions.
fn os_attr(name: &str, args: &TokenStream, aliases: &HashSet<String>) -> bool {
    match name {
        "cfg" => names_os(args, aliases),
        "cfg_attr" => {
            let parts = flat(args.clone());
            let mut parts = parts.split(|tree| punct(Some(tree), ','));
            let predicate: TokenStream = parts.next().unwrap_or_default().iter().cloned().collect();
            names_os(&predicate, aliases)
                || parts.any(|part| {
                    let nested = part.first().and_then(ident).unwrap_or_default();
                    let args = group(part.get(1), Delimiter::Parenthesis).unwrap_or_default();
                    os_attr(&nested, &args, aliases)
                })
        }
        other => other == "windows_subsystem",
    }
}

/// [`os_attr`] on the tokens inside `#[..]`.
fn os_attr_tokens(body: &[TokenTree], aliases: &HashSet<String>) -> bool {
    let name = body.first().and_then(ident).unwrap_or_default();
    let args = group(body.get(1), Delimiter::Parenthesis).unwrap_or_default();
    os_attr(&name, &args, aliases)
}

/// Whether an attribute body hides unused code: `allow(dead_code)` or `expect(dead_code)`, also inside `cfg_attr`.
fn suppresses_dead_code(body: &[TokenTree]) -> bool {
    body.iter().enumerate().any(|(i, tree)| match tree {
        TokenTree::Group(g) => suppresses_dead_code(&flat(g.stream())),
        TokenTree::Ident(id) if id == "allow" || id == "expect" => {
            group(body.get(i + 1), Delimiter::Parenthesis)
                .is_some_and(|args| mentions_ident(args, "dead_code"))
        }
        _ => false,
    })
}

fn mentions_ident(stream: TokenStream, name: &str) -> bool {
    stream.into_iter().any(|tree| match tree {
        TokenTree::Ident(id) => id == name,
        TokenTree::Group(g) => mentions_ident(g.stream(), name),
        _ => false,
    })
}

fn string_value(literal: &proc_macro2::Literal) -> Option<String> {
    syn::parse_str::<syn::LitStr>(&literal.to_string())
        .ok()
        .map(|lit| lit.value())
}

/// `(path, rename)` of every leaf of a `use` tree; a glob's rename is `*`.
fn leaves(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, Option<String>)>,
) {
    let name = |id: &syn::Ident| id.to_string().trim_start_matches("r#").to_string();
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(name(&p.ident));
            leaves(&p.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(n) => {
            let mut path = prefix.clone();
            if n.ident != "self" {
                path.push(name(&n.ident));
            }
            out.push((path, None));
        }
        syn::UseTree::Rename(r) => {
            let mut path = prefix.clone();
            if r.ident != "self" {
                path.push(name(&r.ident));
            }
            out.push((path, Some(name(&r.rename))));
        }
        syn::UseTree::Glob(_) => out.push((prefix.clone(), Some("*".into()))),
        syn::UseTree::Group(g) => g.items.iter().for_each(|item| leaves(item, prefix, out)),
    }
}

/// Whether a resolved path is an OS API.
fn os_path(path: &[String]) -> bool {
    let at = |i: usize| path.get(i).map(String::as_str);
    match at(0) {
        Some("std" | "core" | "alloc") => {
            at(1) == Some("os")
                || (at(1) == Some("env")
                    && at(2) == Some("consts")
                    && matches!(at(3), Some("OS" | "FAMILY")))
        }
        // Local sockets and pipes: Unix sockets, Windows named pipes, and the crate that abstracts them.
        Some("tokio") => {
            (at(1) == Some("net")
                && (at(2).is_some_and(|name| name.starts_with("Unix"))
                    || matches!(at(2), Some("unix" | "windows"))))
                || (at(1) == Some("signal") && matches!(at(2), Some("unix" | "windows")))
        }
        Some("mio") => at(1) == Some("net") && at(2).is_some_and(|name| name.starts_with("Unix")),
        Some("interprocess") => true,
        Some(root) => is_os_crate(root),
        None => false,
    }
}

/// Whether a string is a path that exists on one OS only (`/proc/…`, `C:\…`, a named pipe).
fn os_path_literal(value: &str) -> bool {
    const UNIX_ROOTS: [&str; 14] = [
        "/proc/",
        "/sys/",
        "/dev/",
        "/etc/",
        "/usr/",
        "/bin/",
        "/sbin/",
        "/var/",
        "/run/",
        "/tmp/",
        "/Library/",
        "/System/",
        "/Applications/",
        "/Users/",
    ];
    let bytes = value.as_bytes();
    UNIX_ROOTS.iter().any(|root| value.starts_with(root))
        || value.starts_with("~/Library")
        || value.starts_with("\\\\.\\")
        || value.starts_with("\\\\?\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

/// Free functions and constructors whose argument is a filesystem path.
const FS_FUNCTIONS: [&str; 22] = [
    "read",
    "read_to_string",
    "read_dir",
    "read_link",
    "write",
    "open",
    "create",
    "create_new",
    "create_dir",
    "create_dir_all",
    "remove_file",
    "remove_dir",
    "remove_dir_all",
    "metadata",
    "symlink_metadata",
    "canonicalize",
    "copy",
    "rename",
    "exists",
    "try_exists",
    "set_permissions",
    "hard_link",
];
/// Methods whose argument is a path or a command line.
const PATH_METHODS: [&str; 5] = ["arg", "args", "join", "current_dir", "open"];

/// Shared by every file: names a build script or another module defines for the rest.
#[derive(Default, Clone, PartialEq)]
struct Shared {
    /// OS conditions defined by `cfg_aliases!` (a build script defines what other files use).
    cfg_aliases: HashSet<String>,
    /// Types that wrap a process `Command` (`struct Cmd { command: Command }`, `type Run = Command`).
    command_types: HashSet<String>,
}

fn is_command_type(name: &str, shared: &Shared) -> bool {
    name.ends_with("Command")
        || name.ends_with("CommandWrap")
        || shared.command_types.contains(name)
}

#[derive(Default)]
struct Scan {
    shared: Shared,
    /// Every path each name in this file is imported as (`use libc as sys;` binds `sys` to `libc`).
    imports: HashMap<String, Vec<Vec<String>>>,
    lines: BTreeSet<usize>,
    /// Literals already reported, by position.
    literals: HashSet<(usize, usize)>,
    count: usize,
    /// First pass: only learn names, report nothing.
    collecting: bool,
}

/// The path starting at `trees[at]` (`a::b::<T>::c`): its segments and the index after it.
fn parse_path(trees: &[TokenTree], at: usize) -> (Vec<String>, usize) {
    let mut segments: Vec<String> = ident(&trees[at]).into_iter().collect();
    let mut end = at + 1;
    while path_sep(trees, end) {
        match trees.get(end + 2) {
            Some(tree) if ident(tree).is_some() => {
                segments.extend(ident(tree));
                end += 3;
            }
            Some(TokenTree::Punct(p)) if p.as_char() == '<' => {
                let mut depth = 0;
                let mut k = end + 2;
                while k < trees.len() {
                    if punct(trees.get(k), '<') {
                        depth += 1;
                    } else if punct(trees.get(k), '>') && !(k > 0 && punct(trees.get(k - 1), '-')) {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    k += 1;
                }
                end = (k + 1).min(trees.len());
            }
            _ => break,
        }
    }
    (segments, end)
}

fn any_group(tree: Option<&TokenTree>) -> Option<TokenStream> {
    match tree {
        Some(TokenTree::Group(g)) => Some(g.stream()),
        _ => None,
    }
}

fn first_string(stream: TokenStream) -> Option<String> {
    match stream.into_iter().next() {
        Some(TokenTree::Literal(lit)) => string_value(&lit),
        _ => None,
    }
}

fn cargo_os_variable(value: &str) -> bool {
    value
        .strip_prefix("CARGO_CFG_")
        .is_some_and(|name| OS_ENV_SUFFIXES.contains(&name))
}

/// `#[link(name = "AppKit", kind = "framework")]`, also nested in `cfg_attr`.
fn native_os_link(trees: &[TokenTree]) -> bool {
    trees.iter().enumerate().any(|(i, tree)| match tree {
        TokenTree::Group(g) => native_os_link(&flat(g.stream())),
        TokenTree::Ident(id) if id == "link" => group(trees.get(i + 1), Delimiter::Parenthesis).is_some_and(|args| {
            flat(args).iter().any(|arg| {
                matches!(arg, TokenTree::Literal(lit)
                    if string_value(lit).is_some_and(|kind| matches!(kind.as_str(), "framework" | "raw-dylib")))
            })
        }),
        _ => false,
    })
}

impl Scan {
    fn found(&mut self, tree: &TokenTree) {
        if !self.collecting {
            self.count += 1;
            self.lines.insert(tree.span().start().line);
        }
    }

    /// Every way `segments` can be read once the imports of this file are expanded.
    fn expand(&self, segments: &[String], depth: usize) -> Vec<Vec<String>> {
        let imported = segments.first().and_then(|first| self.imports.get(first));
        match imported {
            Some(paths) if depth > 0 => paths
                .iter()
                .flat_map(|path| {
                    let mut full = path.clone();
                    full.extend_from_slice(&segments[1..]);
                    if full.first() == segments.first() {
                        vec![full]
                    } else {
                        self.expand(&full, depth - 1)
                    }
                })
                .collect(),
            _ => vec![segments.to_vec()],
        }
    }

    fn bind(&mut self, at: &TokenTree, leaves: Vec<(Vec<String>, Option<String>)>) {
        for (path, rename) in leaves {
            if self.expand(&path, 3).iter().any(|full| os_path(full)) {
                self.found(at);
            }
            let name = rename.or_else(|| path.last().cloned());
            if let Some(name) = name.filter(|name| name != "*" && name != "_") {
                let paths = self.imports.entry(name).or_default();
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }

    /// An OS path inside the arguments of a filesystem or command operation counts once per literal.
    fn path_literals(&mut self, trees: &[TokenTree]) {
        for tree in trees {
            match tree {
                TokenTree::Group(g) => self.path_literals(&flat(g.stream())),
                TokenTree::Literal(lit) => {
                    let start = lit.span().start();
                    if string_value(lit).is_some_and(|value| os_path_literal(&value))
                        && (self.collecting || self.literals.insert((start.line, start.column)))
                    {
                        self.found(tree);
                    }
                }
                _ => {}
            }
        }
    }

    fn walk(&mut self, trees: &[TokenTree]) {
        let mut i = 0;
        while i < trees.len() {
            match &trees[i] {
                TokenTree::Group(g) => self.walk(&flat(g.stream())),
                TokenTree::Punct(p) if p.as_char() == '#' => {
                    let at = i + usize::from(punct(trees.get(i + 1), '!')) + 1;
                    if let Some(inner) = group(trees.get(at), Delimiter::Bracket) {
                        let body = flat(inner.clone());
                        let name = body.first().and_then(ident);
                        let cfg = matches!(name.as_deref(), Some("cfg" | "cfg_attr"));
                        // One attribute is one occurrence, however many rules it breaks.
                        if name.as_deref() == Some("windows_subsystem")
                            || native_os_link(&body)
                            || suppresses_dead_code(&body)
                            || (cfg && os_attr_tokens(&body, &self.shared.cfg_aliases))
                        {
                            self.found(&trees[i]);
                        }
                        // Other attributes can carry paths and strings (`doc`): scan them like any tokens.
                        if !cfg {
                            self.walk(&body);
                        }
                        i = at;
                    }
                }
                TokenTree::Ident(_) => {
                    i = self.item(trees, i);
                    continue;
                }
                TokenTree::Literal(_) | TokenTree::Punct(_) => {}
            }
            i += 1;
        }
    }

    /// Handles the identifier at `i`; returns the next index to visit.
    fn item(&mut self, trees: &[TokenTree], i: usize) -> usize {
        let tree = &trees[i];
        let name = ident(tree).unwrap_or_default();
        let bang = punct(trees.get(i + 1), '!');
        if i > 0 && punct(trees.get(i - 1), '.') {
            // A method call: `.arg("/dev/null")`, `.join("C:\\x")`.
            if PATH_METHODS.contains(&name.as_str()) {
                if let Some(args) = group(trees.get(i + 1), Delimiter::Parenthesis) {
                    self.path_literals(&flat(args));
                }
            }
            return i + 1;
        }
        let extern_crate =
            name == "extern" && trees.get(i + 1).and_then(ident).as_deref() == Some("crate");
        if name == "use" || extern_crate {
            let end = trees[i..]
                .iter()
                .position(|t| punct(Some(t), ';'))
                .map_or(trees.len(), |p| i + p);
            let tokens: TokenStream = trees[i..(end + 1).min(trees.len())]
                .iter()
                .cloned()
                .collect();
            if name == "use" {
                if let Ok(item) = syn::parse2::<syn::ItemUse>(tokens) {
                    let mut out = Vec::new();
                    leaves(&item.tree, &mut Vec::new(), &mut out);
                    self.bind(tree, out);
                }
            } else if let Ok(item) = syn::parse2::<syn::ItemExternCrate>(tokens) {
                let rename = item
                    .rename
                    .map(|(_, id)| id.to_string().trim_start_matches("r#").to_string());
                self.bind(tree, vec![(vec![item.ident.to_string()], rename)]);
            }
            return end + 1;
        }
        if matches!(name.as_str(), "struct" | "type") {
            self.learn_command_type(trees, i);
        }
        if bang && name == "cfg" {
            if let Some(inner) = any_group(trees.get(i + 2)) {
                if names_os(&inner, &self.shared.cfg_aliases) {
                    self.found(tree);
                }
                return i + 3;
            }
        }
        if bang && matches!(name.as_str(), "env" | "option_env") {
            if let Some(inner) = any_group(trees.get(i + 2)) {
                if first_string(inner).is_some_and(|value| cargo_os_variable(&value)) {
                    self.found(tree);
                }
                return i + 3;
            }
        }
        let macro_is_cfg_aliases = bang
            && self
                .expand(std::slice::from_ref(&name), 3)
                .iter()
                .any(|path| path.last().is_some_and(|last| last == "cfg_aliases"));
        if macro_is_cfg_aliases {
            if let Some(TokenTree::Group(body)) = trees.get(i + 2) {
                self.define_cfg_aliases(&flat(body.stream()));
                return i + 3;
            }
        }
        if !mid_path(trees, i) {
            self.path(trees, i);
        }
        i + 1
    }

    /// `struct Cmd { command: Command, … }` and `type Run = Command;` make `Cmd`/`Run` a command type.
    fn learn_command_type(&mut self, trees: &[TokenTree], at: usize) {
        let Some(name) = trees.get(at + 1).and_then(ident) else {
            return;
        };
        let mut wraps = false;
        for tree in &trees[at + 2..] {
            match tree {
                TokenTree::Punct(p) if p.as_char() == ';' => break,
                TokenTree::Group(g) => {
                    wraps |= mentions_command(g.stream());
                    if g.delimiter() == Delimiter::Brace {
                        break;
                    }
                }
                TokenTree::Ident(id) => wraps |= is_command_type(&id.to_string(), &self.shared),
                _ => {}
            }
        }
        if wraps {
            self.shared.command_types.insert(name);
        }
    }

    /// A path in expression, type or pattern position, and the call that may follow it.
    fn path(&mut self, trees: &[TokenTree], at: usize) {
        let (segments, end) = parse_path(trees, at);
        let options = self.expand(&segments, 3);
        if segments.len() > 1 && options.iter().any(|path| os_path(path)) {
            self.found(&trees[at]);
        }
        let Some(args) = group(trees.get(end), Delimiter::Parenthesis) else {
            return;
        };
        // The repository's own subprocess helpers take the command line as one string: `sh("uname -a")`.
        if let [helper] = segments.as_slice() {
            if COMMAND_HELPERS.contains(&helper.as_str()) && !self.imports.contains_key(helper) {
                let line = flat(args.clone()).iter().find_map(|tree| match tree {
                    TokenTree::Literal(lit) => string_value(lit),
                    _ => None,
                });
                let word = line
                    .as_deref()
                    .and_then(|line| line.split_whitespace().next());
                if word.is_some_and(os_program) {
                    self.found(&trees[at]);
                }
            }
        }
        let (mut program, mut cargo_var, mut fs_path) = (None, false, false);
        for path in &options {
            let [.., prev, last] = path.as_slice() else {
                continue;
            };
            if matches!(last.as_str(), "new" | "with_new") && is_command_type(prev, &self.shared) {
                program = program.or_else(|| first_string(args.clone()));
            }
            if matches!(last.as_str(), "var" | "var_os")
                && path.len() == 3
                && path[..2] == ["std", "env"]
            {
                cargo_var |=
                    first_string(args.clone()).is_some_and(|value| cargo_os_variable(&value));
            }
            fs_path |= (matches!(prev.as_str(), "fs" | "File" | "OpenOptions")
                && FS_FUNCTIONS.contains(&last.as_str()))
                || (matches!(prev.as_str(), "Path" | "PathBuf" | "OsStr" | "OsString")
                    && matches!(last.as_str(), "new" | "from"));
        }
        if let Some(program) = program {
            if os_program(&program) {
                self.found(&trees[at]);
            }
        }
        if cargo_var {
            self.found(&trees[at]);
        }
        if fs_path {
            self.path_literals(&flat(args));
        }
    }

    /// `cfg_aliases! { name: { condition }, … }`: each alias that names an OS counts, and becomes one.
    fn define_cfg_aliases(&mut self, body: &[TokenTree]) {
        for entry in body.split(|tree| punct(Some(tree), ',')) {
            let Some(alias) = entry.first().and_then(ident) else {
                continue;
            };
            let condition: TokenStream = entry[1..].iter().cloned().collect();
            if names_os(&condition, &self.shared.cfg_aliases) {
                self.found(&entry[0]);
                self.shared.cfg_aliases.insert(alias);
            }
        }
    }
}

fn mentions_command(stream: TokenStream) -> bool {
    stream.into_iter().any(|tree| match tree {
        TokenTree::Ident(id) => {
            let name = id.to_string();
            name.ends_with("Command") || name.ends_with("CommandWrap")
        }
        TokenTree::Group(g) => mentions_command(g.stream()),
        _ => false,
    })
}

/// What one file holds: OS operations (count and lines), plus the names it defines for the others.
struct Report {
    count: usize,
    lines: BTreeSet<usize>,
    shared: Shared,
}

fn scan(source: &str, shared: &Shared) -> Result<Report> {
    let stream = TokenStream::from_str(source).map_err(|e| e.to_string())?;
    let trees = flat(stream);
    let mut scan = Scan {
        shared: shared.clone(),
        collecting: true,
        ..Scan::default()
    };
    // Twice: the second pass sees imports declared after their use, and chains of them.
    scan.walk(&trees);
    scan.walk(&trees);
    scan.collecting = false;
    scan.walk(&trees);
    Ok(Report {
        count: scan.count,
        lines: scan.lines,
        shared: scan.shared,
    })
}

/// Platform code (the `platform` module, `platform.rs`) is where the OS belongs.
fn exempt(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "platform")
        || path.file_name().is_some_and(|name| name == "platform.rs")
}

pub fn check() -> Result<()> {
    inspect(&repo())
}

/// The boundary check over the Rust files of the Git repository at `root`.
fn inspect(root: &Path) -> Result<()> {
    // Git's inventory includes every crate and build script, without target/, vendored caches or worktrees.
    let output = std::process::Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.rs",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("git ls-files failed".into());
    }
    let mut files = Vec::new();
    for path in output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = std::str::from_utf8(path)
            .map_err(|e| e.to_string())?
            .to_string();
        let source =
            std::fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        files.push((path, source));
    }
    // OS aliases and command wrappers are defined in one file and used in others, and may build on each other.
    let mut shared = Shared::default();
    loop {
        let before = shared.clone();
        // Platform files too: an alias defined there (a build module) gates code everywhere.
        for (path, source) in &files {
            let found = scan(source, &shared)
                .map_err(|e| format!("{path}: {e}"))?
                .shared;
            shared.cfg_aliases.extend(found.cfg_aliases);
            shared.command_types.extend(found.command_types);
        }
        if shared == before {
            break;
        }
    }
    let mut errors = Vec::new();
    let mut total = 0;
    for (path, source) in &files {
        if exempt(Path::new(path)) {
            continue;
        }
        let report = scan(source, &shared).map_err(|e| format!("{path}: {e}"))?;
        if report.count > 0 {
            total += report.count;
            let lines: Vec<String> = report.lines.iter().map(usize::to_string).collect();
            errors.push(format!(
                "{path}: {} OS occurrences (lines {}); move OS code into platform",
                report.count,
                lines.join(", ")
            ));
        }
    }
    if errors.is_empty() {
        println!("platform boundary: no OS-specific code outside platform");
        Ok(())
    } else {
        Err(format!(
            "{}\nPlatform boundary: {total} OS occurrences in {} files",
            errors.join("\n"),
            errors.len()
        ))
    }
}

#[cfg(test)]
mod tests;
