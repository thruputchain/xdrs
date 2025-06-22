use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;
use crate::util::*;

pub fn write_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let mut impls = TokenStream::new();

    impls.extend(write_data(input));

    Ok(quote!{
        const _: () = {
            use ::cxs::codec::writer::Result;

            #impls
        };
    })
}

fn write_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(item) =>
            Ok(write_struct(input, item)),
        _ => err(input, "Can't derive for this item")
    }
}

fn write_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> TokenStream {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut fields_write_xdr = TokenStream::new();
    for f in item.fields.iter() {
        fields_write_xdr.extend(write_field(f));
    }

    quote! {
        impl #impl_generics WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<W: Writer>(&self, w: W) -> Result {
                let mut s = w.write_struct()?;

                #fields_write_xdr

                Ok(())
            }
        }
    }
}

fn write_field(f: &syn::Field) -> TokenStream {
    let ident = &f.ident.as_ref().unwrap();

    quote! {
        s.write_field(&(self.#ident))?;
    }
}

pub fn read_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let mut impls = TokenStream::new();

    impls.extend(read_data(input));

    Ok(quote!{
        const _: () = {
            use core::mem::MaybeUninit as MU;
            use core::mem::transmute;
            use ::cxs::codec::reader::Result;

            #impls
        };
    })
}

fn read_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(item) =>
            Ok(read_struct(input, item)),
        _ => err(input, "Can't derive for this item")
    }
}

fn read_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> TokenStream {
    let ty_ident = &input.ident;
    let ty_lifetimes = input.generics.params.iter().filter_map(|g| {
        if let syn::GenericParam::Lifetime(param) = g {
            Some(&param.lifetime)
        } else {
            None
        }
    });
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut lifetime_param: syn::LifetimeParam = syn::parse_quote!{ '_r };
    for lt in ty_lifetimes {
        // Add each of the item's lifetime params as a bound on the reader lifetime
        lifetime_param.bounds.push(lt.clone())
    }

    let mut generics = input.generics.clone();
    generics.params.push(syn::GenericParam::Lifetime(lifetime_param));
    let (impl_generics, _ty_generics, _where_clause) = generics.split_for_impl();

    let mut fields_read_xdr = TokenStream::new();
    for f in item.fields.iter() {
        fields_read_xdr.extend(read_field(f));
    }

    quote! {
        impl #impl_generics ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr(value: &mut MU<Self>, r: &mut dyn Reader<'_r>) -> Result<()> {
                let struct_ptr = value.as_mut_ptr();

                #fields_read_xdr

                Ok(())
            }
        }
    }
}

fn read_field(f: &syn::Field) -> TokenStream {
    let ident = &f.ident.as_ref().unwrap();
    let ty = &f.ty;

    quote! {
        let field_mu: &mut MU<#ty> = unsafe {
            transmute(&raw mut (*struct_ptr).#ident)
        };

        read_xdr(field_mu, r)?;
    }
}
