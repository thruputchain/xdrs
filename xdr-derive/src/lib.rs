//! Generate traits ReadXDR, WriteXDR, XDR

mod generate;
mod generate_type;
mod util;

use proc_macro::TokenStream;

#[proc_macro_derive(WriteXDR, attributes(xdrs))]
pub fn derive_write_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate::write_xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(ReadXDR, attributes(xdrs))]
pub fn derive_read_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate::read_xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(XDR, attributes(xdr))]
pub fn derive_xdr(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate_type::xdr(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro]
pub fn xdr(input: TokenStream) -> TokenStream {
    use xdr_repr::*;
    use quote::ToTokens;
    // use xdr_repr::to_tokens::*;

    let spec: Specification = match syn::parse(input) {
        Ok(spec) => spec,
        Err(e) => return syn::Error::into_compile_error(e).into()
    };
    spec.into_token_stream().into()

        /*
    let spec_string = format!("{:?}", spec);
    let output: syn::Expr = syn::parse_str(&spec_string).unwrap();
    // let output = quote! { #spec_string };

    output.into_token_stream().into()
        */
}
