use xdr_repr::xdr::*;
use proc_macro2::TokenStream;

fn tokens_from_file(path: &str) -> std::io::Result<TokenStream> {
    use std::fs;
    use std::io::{Error, ErrorKind};

    let contents = fs::read_to_string(path)?;

    contents.parse()
        .map_err(|e| Error::new(ErrorKind::InvalidData, format!("{}", e)))
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.len() != 1 {
        eprintln!("usage: parse <PATH>");
        std::process::exit(1);
    }
    let path = args.next().unwrap();

    let tokens = tokens_from_file(path.as_str()).unwrap();

    let parsed: Specification = syn::parse2(tokens).unwrap();

    println!("{}", parsed);
}
