//! Parses every `.x` file in the directories listed, separated by `:`, in
//! `XDRC_CORPUS`, such as a checkout of thruput-protocol. Without
//! `XDRC_CORPUS`, the test does nothing.

use std::path::{Path, PathBuf};

use xdr_codegen::diag::LineIndex;

fn x_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            x_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "x") {
            out.push(path);
        }
    }
}

#[test]
fn corpus_parses() {
    let Ok(dirs) = std::env::var("XDRC_CORPUS") else {
        eprintln!("XDRC_CORPUS is not set; skipping");
        return;
    };
    let mut files = Vec::new();
    for dir in dirs.split(':').filter(|d| !d.is_empty()) {
        x_files(Path::new(dir), &mut files);
    }
    assert!(!files.is_empty(), "no .x files in {dirs}");

    let mut failed = 0;
    for path in &files {
        let src = std::fs::read_to_string(path).unwrap();
        let lines = LineIndex::new(&src);
        let file = path.display().to_string();
        match xdr_codegen::parse(&src) {
            Ok(parsed) => {
                for w in &parsed.warnings {
                    eprintln!("{}", w.display(&file, &lines));
                }
            }
            Err(diags) => {
                failed += 1;
                for d in &diags {
                    eprintln!("{}", d.display(&file, &lines));
                }
            }
        }
    }
    eprintln!("parsed {} files, {failed} failed", files.len());
    assert_eq!(failed, 0);
}
