use xdr_repr::xdr::*;
use proc_macro2::TokenStream;

fn main() {
    let mut args = std::env::args().skip(1);
    if args.len() != 1 {
        eprintln!("usage: parse <PATH>");
        std::process::exit(1);
    }
    let path = args.next().unwrap();

    let tokens = tokens_from_file(path.as_str()).unwrap();

    let parsed: Namespace = syn::parse2(tokens).unwrap();

    println!("{}", parsed);
}

fn tokens_from_file(path: &str) -> std::io::Result<TokenStream> {
    use std::fs;
    use std::io::{Error, ErrorKind};

    let contents = fs::read_to_string(path)?;

    // Syntax like "10'000'000" can't be parsed to a TokenStream
    let contents = remove_int_separators(&*contents);

    contents.parse()
        .map_err(|e| Error::new(ErrorKind::InvalidData, format!("{}", e)))
}

fn remove_int_separators(s: &str) -> String {
    let mut out = String::with_capacity(s.len());

    let mut c_prev = ' ';
    for c in s.chars() {
        if c == '\'' && c_prev.is_ascii_digit() {
            continue;
        }

        c_prev = c;

        out.push(c);
    }

    out
}
