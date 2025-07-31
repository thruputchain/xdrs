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

#[proc_macro_derive(HasTypeDef)]
pub fn derive_has_type_def(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    generate_type::has_type_def(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
