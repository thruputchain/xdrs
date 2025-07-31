use crate::util::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

pub fn xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let imp = from_data(input)?;
    Ok(quote! {
        const _: () = {
            mod __xdrs {
                pub use ::xdr_repr::ty::*;
                pub use ::xdr_repr::xdr::*;
                pub use ::xdr_repr::construct::*;
            }

            #imp
        };
    })
}

fn from_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(item) => Ok(from_struct(input, item)),
        // syn::Data::Enum(item) => Ok(from_enum(input, item)),
        _ => err(input, "Can't derive for this item"),
    }
}

fn from_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> TokenStream {
    let ty_ident = &input.ident;
    let ty_name = ty_ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let declare_fields = token_reduce(&item.fields, declare_field);
    quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                __xdrs::struct_def(#ty_name, vec![
                    #declare_fields
                ])
            }
        }
    }
}

fn declare_field(_index: usize, field: &syn::Field) -> TokenStream {
    let ty = &field.ty;
    let ident = field.ident.as_ref().unwrap();
    let name = ident.to_string();
    // let fa = FieldAccess::new(index, field);
    // let attrs = FieldAttrs::from_field(field).unwrap();

    quote! {
        {
            <#ty as __xdrs::Declare>::declare(
                __xdrs::ident(#name))
        },
    }
}
