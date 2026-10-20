//! Stamps the build with its version and git commit, so a running service can
//! say exactly what it was built from.
//!
//! Versions are calendar dates. Cargo.toml holds 26.9.24 for a release on
//! 2026-09-24, or 26.9.24+2 for a second release that day, since Cargo only
//! accepts three numbers without leading zeros. They are shown as v26.09.24
//! and v26.09.24.2.
//!
//! The same file lives in banner_eye, bpm and websocket_server.

use std::path::Path;
use std::process::Command;

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    println!("cargo:rustc-env=APP_VERSION={}", display_version(&version));
    println!("cargo:rustc-env=GIT_COMMIT={}", git_commit());

    // Rerun when the version or the checked-out commit changes, rather than on
    // every source edit. The index changes on a pull, a checkout or an edit
    // being staged, which is what moves the "modified" mark.
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    if let Ok(head) = std::fs::read_to_string(".git/HEAD") {
        if let Some(branch) = head.trim().strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=.git/{branch}");
        }
    }
}

/// 26.9.24 as v26.09.24, and 26.9.24+2 as v26.09.24.2. Anything else is shown
/// as it is.
fn display_version(version: &str) -> String {
    let (date, release) = match version.split_once('+') {
        Some((date, release)) => (date, Some(release)),
        None => (version, None),
    };
    let parts: Vec<u32> = date.split('.').filter_map(|p| p.parse().ok()).collect();
    match (parts.as_slice(), release) {
        ([y, m, d], release) if *y >= 20 && (1..=12).contains(m) && (1..=31).contains(d) => {
            match release {
                None => format!("v{y:02}.{m:02}.{d:02}"),
                Some(n) => format!("v{y:02}.{m:02}.{d:02}.{n}"),
            }
        }
        _ => format!("v{version}"),
    }
}

/// The short commit hash, marked "-modified" when tracked files differ from
/// it. "unknown" when built outside a git checkout.
fn git_commit() -> String {
    if !Path::new(".git").exists() {
        return "unknown".to_string();
    }
    let git = |args: &[&str]| {
        // --no-optional-locks stops git status refreshing the index, which
        // would otherwise touch .git/index and rerun this script every build.
        Command::new("git")
            .arg("--no-optional-locks")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let Some(hash) = git(&["rev-parse", "--short", "HEAD"]) else {
        return "unknown".to_string();
    };
    let modified =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    if modified {
        format!("{hash}-modified")
    } else {
        hash
    }
}
