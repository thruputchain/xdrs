use pest::Parser;
use xdr_repr::parse::*;
use std::fs;

#[test]
fn parse_simple() {
    assert!(matches!(XDRParser::parse(Rule::field, "-273.15"), Ok(_)));

    assert!(matches!(XDRParser::parse(Rule::field, "this is not a number"), Err(_)));
}

#[test]
fn parse_file() {
    let unparsed_file = fs::read_to_string("numbers.csv").expect("cannot read file");

    let file = XDRParser::parse(Rule::file, &unparsed_file)
        .expect("unsuccessful parse")
        .next().unwrap(); // get and unwrap the `file` rule; never fails

}

