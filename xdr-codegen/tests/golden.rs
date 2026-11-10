//! Checks that the generated modules committed in `check/src` are what the
//! generator produces now. The `check` crate compiles and tests them against
//! this workspace's `xdrs` and `thruput-rpc-api`.
//!
//! To update them after changing the generator, run this test with
//! `XDRC_BLESS=1` and review the diff.

use std::path::Path;

use xdr_codegen::rust::Options;

fn options(on: &[&str], off: &[&str]) -> Options {
    Options {
        features_on: on.iter().map(|s| s.to_string()).collect(),
        features_off: off.iter().map(|s| s.to_string()).collect(),
        ..Options::default()
    }
}

fn check(input: &str, output: &str, options: &Options) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("check");
    let src = std::fs::read_to_string(dir.join(input)).unwrap();
    let parsed = xdr_codegen::parse(&src).unwrap_or_else(|d| panic!("{input}: {d:?}"));
    let code = xdr_codegen::rust::generate(&parsed.spec, options)
        .unwrap_or_else(|d| panic!("{input}: {d:?}"));
    // Leave the commit out of the header, so that the committed files do
    // not change with every commit.
    let code = code.replacen(xdr_codegen::VERSION, env!("CARGO_PKG_VERSION"), 1);
    let path = dir.join("src").join(output);
    if std::env::var_os("XDRC_BLESS").is_some() {
        std::fs::write(&path, code).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == code,
        "{} is out of date; rerun with XDRC_BLESS=1 and review the diff",
        path.display()
    );
}

#[test]
fn check_x() {
    // As generated crates use it.
    check(
        "check.x",
        "generated.rs",
        &options(&["xdrs", "schema"], &["spec"]),
    );
}

#[test]
fn plain_x() {
    // With every derive feature off.
    check(
        "plain.x",
        "plain.rs",
        &options(&[], &["xdrs", "spec", "schema", "serde"]),
    );
}
