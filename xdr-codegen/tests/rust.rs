//! What the Rust backend rejects, and details of what it emits that the
//! `check` crate does not show.

use xdr_codegen::diag::LineIndex;
use xdr_codegen::rust::{Options, generate};

fn rust(src: &str) -> Result<String, String> {
    let spec = xdr_codegen::parse(src).expect("parses").spec;
    let lines = LineIndex::new(src);
    generate(&spec, &Options::default()).map_err(|diags| {
        diags
            .iter()
            .map(|d| format!("{}\n", d.display("t.x", &lines)))
            .collect()
    })
}

fn error(src: &str) -> String {
    rust(src).expect_err("generated")
}

#[test]
fn rejects() {
    assert_eq!(
        error("struct S { quadruple q; };\n"),
        "t.x:1:12: error: `quadruple` has no Rust type\n"
    );
    assert_eq!(
        error("union U switch (hyper h) { case 1: void; };\n"),
        "t.x:1:23: error: a union can switch only on `int`, `unsigned int`, `bool` or an enum\n"
    );
    assert_eq!(
        error("union U switch (bool b) { case 1: void; };\n"),
        "t.x:1:32: error: a union switched on `bool` takes only `TRUE` and `FALSE`\n"
    );
    assert_eq!(
        error("struct S { int self; };\n"),
        "t.x:1:16: error: `self` cannot be a Rust identifier\n"
    );
    assert_eq!(
        error("struct S { opaque a<-1>; };\n"),
        "t.x:1:21: error: a length cannot be negative\n"
    );
}

#[test]
fn rust_keywords_are_raw() {
    let code = rust("struct S { int type; int match; };\n").unwrap();
    assert!(code.contains("pub r#type: i32"), "{code}");
    assert!(code.contains("pub r#match: i32"), "{code}");
}

#[test]
fn numbers_keep_their_form() {
    let code = rust("const A = 1'000'000;\nconst B = 0xFF;\nconst C = 010;\n").unwrap();
    assert!(code.contains("pub const A: u32 = 1_000_000;"), "{code}");
    assert!(code.contains("pub const B: u32 = 0xFF;"), "{code}");
    // A leading zero is octal in XDR, but not in Rust.
    assert!(code.contains("pub const C: u32 = 0o10;"), "{code}");
}

#[test]
fn comments() {
    let code =
        rust("/// Doc.\n// Plain.\n// #[rust(newtype)]\n// #[doc(hidden)]\ntypedef int T;\n")
            .unwrap();
    assert!(
        code.contains("/// Doc.\n// Plain.\n#[doc(hidden)]\n#[cfg_attr"),
        "{code}"
    );
    assert!(!code.contains("#[rust("), "{code}");
}

#[test]
fn union_case_names() {
    let code = rust(
        "const K = 7;\nunion U switch (Kind k) {\n  case A: void;\n  case -1: void;\n  case 0x10: void;\n  case K: void;\n};\n",
    )
    .unwrap();
    // A name that is not a constant of the file is an enum member.
    assert!(code.contains("A = Kind::A.0 as i32,"), "{code}");
    assert!(code.contains("_neg1 = -1 as Kind as i32,"), "{code}");
    assert!(code.contains("_0x10 = 0x10 as Kind as i32,"), "{code}");
    assert!(code.contains("_K = K as Kind as i32,"), "{code}");
}

#[test]
fn features_can_be_forced() {
    let spec = xdr_codegen::parse("struct S { int a; };\n").unwrap().spec;
    let options = Options {
        features_on: ["xdrs".to_string()].into(),
        features_off: ["serde".to_string(), "spec".to_string()].into(),
        ..Options::default()
    };
    let code = generate(&spec, &options).unwrap();
    assert!(
        code.contains("#[derive(ReadXDR, WriteXDR, ConstXDRSize)]"),
        "{code}"
    );
    assert!(!code.contains("serde"), "{code}");
    assert!(!code.contains("XDR)]"), "{code}");
    assert!(code.contains("#[cfg_attr(feature = \"schema\""), "{code}");
}
