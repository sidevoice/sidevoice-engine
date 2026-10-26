use super::*;

fn n(source: &str) -> usize {
    scan(source, &Shared::default()).unwrap().count
}

#[test]
fn catches_os_cfgs_and_apis_in_any_attribute_or_macro_form() {
    assert_eq!(
        n("#[cfg_attr(\n all(target_os = \"linux\", unix), ignore)]\nuse std::os::unix::fs::PermissionsExt;"),
        2
    );
    assert_eq!(
        n("fn f() { if cfg!(windows) {} let _ = std::env::consts::OS; }"),
        2
    );
    assert_eq!(
        n("fn f() { let _ = cfg! { unix }; let _ = cfg![windows]; }"),
        2
    );
    assert_eq!(n("#![cfg(target_os = \"macos\")]"), 1);
    assert_eq!(n("fn f() { unsafe { libc::getpid() }; }"), 1);
}

#[test]
fn web_is_not_an_os_and_comments_and_literals_are_not_code() {
    let source = r##"
        //! std::os::fd cfg!(windows)
        #[cfg(target_family = "wasm")]
        #[cfg(target_arch = "wasm32")]
        #[cfg_attr(not(target_family = "wasm"), derive(Debug))]
        fn host() { let _ = "libc::x cfg!(unix)"; let _ = r#"Command::new("sh")"#; }
        fn other(v: &[u8]) { let _ = v.windows(2); let _ = cfg!(feature = "windows"); }
    "##;
    assert_eq!(n(source), 0);
    assert_eq!(
        n("#[cfg(any(target_family = \"wasm\", unix))] fn f() {}"),
        1
    );
}

#[test]
fn catches_every_framework_family() {
    for path in [
        "cocoa::appkit::NSApp",
        "objc::runtime::Object",
        "objc2::rc::Retained",
        "objc2_foundation::NSString",
        "objc2_app_kit::NSAlert",
        "core_foundation::string::CFString",
        "core_foundation_sys::base::CFRelease",
        "core_graphics::display::CGDisplay",
        "windows_sys::Win32::Foundation::CloseHandle",
        "windows::Win32::Foundation::HANDLE",
        "windows_core::PCWSTR",
        "rustix::fs::chown",
        "nix::unistd::getpid",
    ] {
        assert_eq!(n(&format!("fn f() {{ {path}; }}")), 1, "{path}");
        let (krate, rest) = path.split_once("::").unwrap();
        assert_eq!(n(&format!("use {krate}::{rest};")), 1, "use {path}");
    }
    assert_eq!(
        n("use ordinary::objc2_like; fn f() { ordinary::cocoa(); }"),
        0
    );
}

#[test]
fn grouped_nested_and_renamed_imports_keep_the_os_path() {
    assert_eq!(n("use std::{os::unix::fs::PermissionsExt};"), 1);
    assert_eq!(n("use std::{fs, os::{unix::fs::PermissionsExt, raw}};"), 2);
    assert_eq!(n("use libc as sys; fn f() { sys::getpid(); }"), 2);
    assert_eq!(
        n("use objc2_app_kit as kit; fn f() { kit::NSAlert::new(); }"),
        2
    );
    assert_eq!(n("extern crate libc as l; fn f() { l::getpid(); }"), 2);
    assert_eq!(n("use std::{io, fs}; use ordinary as libc;"), 0);
}

#[test]
fn resolves_cfg_aliases_across_files() {
    let build = "fn main() { cfg_aliases::cfg_aliases! { web: { target_arch = \"wasm32\" }, \
        native: { not(target_arch = \"wasm32\") }, \
        apple_silicon: { all(target_os = \"macos\", target_arch = \"aarch64\") }, \
        silicon_native: { all(apple_silicon, native) } } }";
    let aliases = scan(build, &Shared::default()).unwrap();
    assert_eq!(aliases.count, 2);
    assert_eq!(
        aliases.shared.cfg_aliases,
        HashSet::from(["apple_silicon".to_string(), "silicon_native".to_string()])
    );
    let user = "#[cfg(apple_silicon)] mod a {} #[cfg(native)] mod b {} #[cfg(web)] mod c {} \
        #[cfg_attr(silicon_native, allow(dead_code))] fn d() { if cfg!(apple_silicon) {} }";
    assert_eq!(scan(user, &aliases.shared).unwrap().count, 3);
}

#[test]
fn catches_os_commands() {
    assert_eq!(
        n("fn f() { std::process::Command::new(\"uname\").arg(\"-a\"); }"),
        1
    );
    assert_eq!(
        n("fn f() { tokio::process::Command::new(\"/bin/sh\"); }"),
        1
    );
    assert_eq!(n("fn f() { Command::new(\"PowerShell.exe\"); }"), 1);
    assert_eq!(
        n("use std::process::Command as Cmd; fn f() { Cmd::new(\"launchctl\"); }"),
        1
    );
    assert_eq!(
        n("fn f() { Command::new(\"git\"); Command::new(\"cargo\"); }"),
        0
    );
    let shared = Shared {
        command_types: HashSet::from(["Cmd".to_string()]),
        ..Shared::default()
    };
    assert_eq!(
        scan("fn f() { Cmd::new(\"sysctl\"); }", &shared)
            .unwrap()
            .count,
        1
    );
}

#[test]
fn catches_cargo_os_variables_and_native_links() {
    assert_eq!(
        n("fn main() { let _ = std::env::var(\"CARGO_CFG_TARGET_OS\"); }"),
        1
    );
    assert_eq!(n("fn main() { let _ = env!(\"CARGO_CFG_UNIX\"); }"), 1);
    assert_eq!(
        n("#[link(name = \"AppKit\", kind = \"framework\")] extern \"C\" {}"),
        1
    );
    assert_eq!(
        n("fn main() { let _ = std::env::var(\"CARGO_CFG_TARGET_ARCH\"); }"),
        0
    );
}

#[test]
fn exempts_only_the_platform_module() {
    assert!(exempt(Path::new("src/platform/links/unix.rs")));
    assert!(exempt(Path::new("src/platform.rs")));
    assert!(exempt(Path::new("xtask/src/platform.rs")));
    assert!(!exempt(Path::new("tests/platform_test.rs")));
}

#[test]
fn rejects_unreadable_source() {
    assert!(scan("fn f() { \"unterminated }", &Shared::default()).is_err());
}

#[test]
fn imports_are_followed_through_renames_and_chains_in_any_order() {
    assert_eq!(
        n("use s::os::unix::fs::symlink;\nuse std as s;\nfn f() { let _ = symlink(\"a\", \"b\"); }"),
        1
    );
    assert_eq!(n("use std as s;\nuse s::path::Path;\nfn f(_: &Path) {}"), 0);
    let aliases = "use cfg_aliases::cfg_aliases as aliases;\nfn main() { aliases! { apple_silicon: { all(target_os = \"macos\", target_arch = \"aarch64\") }, web: { target_family = \"wasm\" } } }";
    let report = scan(aliases, &Shared::default()).unwrap();
    assert_eq!(report.count, 1);
    assert!(report.shared.cfg_aliases.contains("apple_silicon"));
    assert!(!report.shared.cfg_aliases.contains("web"));
    let shared = Shared {
        cfg_aliases: HashSet::from(["apple_silicon".to_string()]),
        ..Shared::default()
    };
    assert_eq!(
        scan("#[cfg(apple_silicon)] fn f() {}", &shared)
            .unwrap()
            .count,
        1
    );
}

#[test]
fn dead_code_suppressions_and_os_command_helpers_are_seen() {
    assert_eq!(n("#[allow(dead_code)] fn unavailable() {}"), 1);
    assert_eq!(n("#[expect(dead_code)] fn unavailable() {}"), 1);
    assert_eq!(
        n("#[cfg_attr(feature = \"x\", allow(dead_code))] fn f() {}"),
        1
    );
    assert_eq!(n("#[allow(clippy::too_many_arguments)] fn f() {}"), 0);
    assert_eq!(n("fn f() { let _ = sh(\"uname -a\"); }"), 1);
    assert_eq!(
        n("fn f() { let _ = run_in(&dir, \"powershell -c x\", &[]); }"),
        1
    );
    assert_eq!(n("fn f() { let _ = sh(\"cargo metadata\"); }"), 0);
}
