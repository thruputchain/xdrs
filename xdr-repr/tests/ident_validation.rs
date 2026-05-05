use xdr_repr::ident;

#[test]
fn valid_identifiers_accepted() {
    let _ = ident("foo");
    let _ = ident("_bar");
    let _ = ident("Baz123");
    let _ = ident("UPPER_CASE");
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn raw_identifier_prefix_rejected() {
    let _ = ident("r#type");
}

#[test]
#[should_panic(expected = "XDR identifier cannot be empty")]
fn empty_identifier_rejected() {
    let _ = ident("");
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn identifier_starting_with_digit_rejected() {
    let _ = ident("9bad");
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn identifier_with_dash_rejected() {
    let _ = ident("bad-name");
}
