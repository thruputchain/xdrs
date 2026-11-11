//! Records the xdrs commit this crate is built from, as `XDRC_COMMIT`, for
//! the header of generated files.
//!
//! The commit is `git rev-parse HEAD`, which also works for `cargo install
//! --git`, whose checkouts keep `.git`, with `-dirty` added if this crate's
//! own files have uncommitted changes. Without git, it is the commit cargo
//! records in `.cargo_vcs_info.json` when packaging, or else `unknown`.

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

    let commit = match git(&["rev-parse", "HEAD"]) {
        Some(head) => {
            // A commit, a checkout or `git add` changes one of these.
            for name in ["HEAD", "index", "packed-refs"] {
                if let Some(path) = git(&["rev-parse", "--git-path", name]) {
                    println!("cargo:rerun-if-changed={path}");
                }
            }
            if let Some(path) = git(&["symbolic-ref", "-q", "HEAD"])
                .and_then(|r| git(&["rev-parse", "--git-path", &r]))
            {
                println!("cargo:rerun-if-changed={path}");
            }
            let dirty = git(&["status", "--porcelain", "--", "."]).is_some_and(|s| !s.is_empty());
            if dirty { format!("{head}-dirty") } else { head }
        }
        None => vcs_info_commit().unwrap_or_else(|| "unknown".into()),
    };
    println!("cargo:rustc-env=XDRC_COMMIT={commit}");
}

/// The commit in `.cargo_vcs_info.json`, which `cargo package` writes.
fn vcs_info_commit() -> Option<String> {
    let path = Path::new(".cargo_vcs_info.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(path).ok()?;
    let rest = &text[text.find("\"sha1\"")? + "\"sha1\"".len()..];
    let start = rest.find('"')? + 1;
    let len = rest[start..].find('"')?;
    Some(rest[start..start + len].to_string())
}
