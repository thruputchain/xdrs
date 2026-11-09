//! What `derive(XDR)` gives the generated types describes the same wire
//! format as `check.x`.

use std::collections::BTreeSet;

use xdr_codegen::repr::{lower, wire_shapes};

#[test]
fn derived_spec_matches_the_source_on_the_wire() {
    let parsed = xdr_codegen::parse(include_str!("../check.x")).expect("check.x parses");
    let source = lower(&parsed.spec).expect("check.x lowers");
    let derived = thruput_xdrc_check::xdr_spec();

    let source = wire_shapes(&source).expect("the source reduces");
    let derived = wire_shapes(&derived).expect("the derived spec reduces");
    assert_eq!(source.errors, Default::default());
    assert_eq!(derived.errors, Default::default());
    let (source_shapes, derived_shapes) = (source.shapes, derived.shapes);

    let mut compared = BTreeSet::new();
    let mut mismatches = Vec::new();
    for (name, shape) in &source_shapes {
        match derived_shapes.get(name) {
            Some(derived) if derived == shape => {
                compared.insert(name.as_str());
            }
            Some(derived) => mismatches.push(format!(
                "{name}:\n  source  {shape:?}\n  derived {derived:?}"
            )),
            // A typedef that becomes a Rust type alias has no definition
            // of its own.
            None => {}
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));

    // Every type but the aliases is compared.
    let aliases = ["Amount", "Code", "Flag", "Index", "T", "TxBatch", "Verdict"];
    let expected: BTreeSet<&str> = source_shapes
        .keys()
        .map(String::as_str)
        .filter(|n| !aliases.contains(n))
        .collect();
    assert_eq!(compared, expected);
}

/// The names of the types whose wire shapes differ between `src` and the
/// derived spec.
fn mismatched(src: &str) -> Vec<String> {
    let parsed = xdr_codegen::parse(src).expect("parses");
    let source = wire_shapes(&lower(&parsed.spec).expect("lowers"))
        .expect("reduces")
        .shapes;
    let derived = wire_shapes(&thruput_xdrc_check::xdr_spec())
        .expect("reduces")
        .shapes;
    source
        .iter()
        .filter(|(name, shape)| derived.get(*name).is_some_and(|d| d != *shape))
        .map(|(name, _)| name.clone())
        .collect()
}

#[test]
fn a_change_on_the_wire_is_caught() {
    let src = include_str!("../check.x");
    for (from, to, culprits) in [
        // A bound.
        ("opaque memo<16>", "opaque memo<17>", &["Receipt", "Tx"][..]),
        // A field's width.
        ("unsigned hyper seq", "unsigned int seq", &["Receipt", "Tx"]),
        // A case value.
        (
            "case 4294967295:\n  int v;\ncase 1:",
            "case 4294967294:\n  int v;\ncase 1:",
            &["ByIndex"],
        ),
    ] {
        assert!(src.contains(from), "{from}");
        assert_eq!(
            mismatched(&src.replacen(from, to, 1)),
            culprits,
            "{from} -> {to}"
        );
    }
}
