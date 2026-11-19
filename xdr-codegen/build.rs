//! Records the xdrs commit this crate is built from, as `XDRC_COMMIT`, for
//! the header of generated files.
//!
//! A packaged crate, as installed from crates.io, has the commit cargo
//! recorded in `.cargo_vcs_info.json` when packaging it. Otherwise the
//! commit is `git rev-parse HEAD`, which also works for `cargo install
//! --git`, whose checkouts keep `.git`, with `-dirty` added if this crate's
//! own files have uncommitted changes; without git, it is `unknown`.
//!
//! The recorded commit comes first because git cannot tell a packaged
//! crate's own repository: in an unpacked crate, it finds whatever
//! repository encloses cargo's registry, if any, and reports that
//! repository's commit.

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // For `-dirty`: any change to this crate's files.
    println!("cargo:rerun-if-changed=.");

    let commit = vcs_info_commit()
        .or_else(git_commit)
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=XDRC_COMMIT={commit}");
}

/// `git rev-parse HEAD`, with `-dirty` if this crate's files have
/// uncommitted changes.
fn git_commit() -> Option<String> {
    let head = git(&["rev-parse", "HEAD"])?;
    // A commit, a checkout or `git add` changes one of these.
    for name in ["HEAD", "index", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(path) =
        git(&["symbolic-ref", "-q", "HEAD"]).and_then(|r| git(&["rev-parse", "--git-path", &r]))
    {
        println!("cargo:rerun-if-changed={path}");
    }
    let dirty = git(&["status", "--porcelain", "--", "."]).is_some_and(|s| !s.is_empty());
    Some(if dirty { format!("{head}-dirty") } else { head })
}

/// The commit in `.cargo_vcs_info.json`, which `cargo package` writes.
fn vcs_info_commit() -> Option<String> {
    let path = Path::new(".cargo_vcs_info.json");
    // Only a packaged crate has the file, and it comes and goes with the
    // package; watching a missing path would rerun this script on every
    // build of a checkout.
    let text = std::fs::read_to_string(path).ok()?;
    println!("cargo:rerun-if-changed={}", path.display());
    let rest = &text[text.find("\"sha1\"")? + "\"sha1\"".len()..];
    let start = rest.find('"')? + 1;
    let len = rest[start..].find('"')?;
    Some(rest[start..start + len].to_string())
}
