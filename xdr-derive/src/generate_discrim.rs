//! `#[derive(XDRDiscriminant)]`: lets an XDR enum switch a union.

use crate::attrs::*;
use crate::util::*;
use darling::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

pub fn xdr_discriminant(input: &syn::DeriveInput) -> Result<TokenStream> {
    let attrs = InputAttrs::from_derive_input(input)?;
    if attrs.discriminant.is_some() {
        return err(
            input,
            "derive XDRDiscriminant: an enum with `#[xdr(discriminant)]` is a \
             union, which cannot switch another union",
        );
    }
    let krate = attrs.crate_path();

    let body = match &input.data {
        syn::Data::Enum(item) => fieldless_enum(input, item)?,
        syn::Data::Struct(item) => newtype(input, item)?,
        syn::Data::Union(_) => {
            return err(
                input,
                "derive XDRDiscriminant: expected a fieldless `#[repr(i32)]` \
                 enum or a newtype over `i32`, not a union",
            );
        }
    };

    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        const _: () = {
            mod __xdrs {
                pub use #krate::discrim::{DiscriminantEncoding, XDRDiscriminant};
            }

            impl #impl_generics __xdrs::XDRDiscriminant for #ty_ident #ty_generics #where_clause {
                // XDR enums are `int`s.
                const ENCODING: __xdrs::DiscriminantEncoding =
                    __xdrs::DiscriminantEncoding::Signed;

                #body
            }
        };
    })
}

/// A closed enum: its tags are its members' values, and any other tag has no
/// value.
fn fieldless_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    if let Some(v) = item.variants.iter().find(|v| !variant_is_fieldless(v)) {
        return err(
            v,
            "derive XDRDiscriminant: an enum's variants must all be fieldless, \
             as an XDR enum's members are",
        );
    }

    let vars: Vec<_> = item.variants.iter().map(|v| &v.ident).collect();

    Ok(quote! {
        fn discriminant(&self) -> i32 {
            match *self {
                #(Self::#vars => Self::#vars as i32,)*
            }
        }

        fn from_discriminant(d: i32) -> ::core::option::Option<Self> {
            match d {
                #(d if d == Self::#vars as i32 => ::core::option::Option::Some(Self::#vars),)*
                _ => ::core::option::Option::None,
            }
        }
    })
}

/// An open enum, a newtype over `i32` with its members as associated
/// constants: every tag is a value.
fn newtype(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    match &item.fields {
        syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {}
        _ => {
            return err(
                input,
                "derive XDRDiscriminant: a struct must be a newtype over `i32`, \
                 such as `struct Color(pub i32);`",
            );
        }
    }

    Ok(quote! {
        fn discriminant(&self) -> i32 {
            self.0
        }

        fn from_discriminant(d: i32) -> ::core::option::Option<Self> {
            ::core::option::Option::Some(Self(d))
        }
    })
}
