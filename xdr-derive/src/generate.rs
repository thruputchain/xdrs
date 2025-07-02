use crate::util::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;
use syn::spanned::Spanned;

use darling::FromField;

#[derive(Debug, FromField)]
#[darling(attributes(xdrs))]
struct FieldAttrs {
    pub as_type: Option<syn::Type>
}

// ---- derive WriteXDR ------------------------------------------------------

pub fn write_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let mut impls = TokenStream::new();

    impls.extend(write_data(input));

    Ok(quote! {
        const _: () = {
            use ::xdrs::serialize::writer::{
                Writer,
                StructWriter,
                EnumWriter,
                Result};
            use ::xdrs::discrim::XDRDiscriminant;

            #impls
        };
    })
}

fn write_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(item) => Ok(write_struct(input, item)),
        syn::Data::Enum(item) => Ok(write_enum(input, item)),
        _ => err(input, "Can't derive for this item"),
    }
}

fn write_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> TokenStream {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let n_fields = item.fields.len();
    let write_fields = token_reduce(&item.fields, write_field);
    quote! {
        impl #impl_generics WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result {
                let mut s = w.write_struct(#n_fields)?;

                #write_fields

                Ok(())
            }
        }
    }
}

fn write_field(index: usize, field: &syn::Field) -> TokenStream {
    let fa = FieldAccess::new(index, field);
    let attrs = FieldAttrs::from_field(field).unwrap();

    if let Some(ty) = attrs.as_type {
        quote! {
            let v: &#ty = self.#fa.deref();
            s.write_field(v)?;
        }
    } else {
        quote! {
            s.write_field(&(self.#fa))?;
        }
    }
}

fn write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> TokenStream {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_variants = token_reduce(&item.variants, |i, v| write_variant(i, v, input));

    quote! {
        impl #impl_generics XDRDiscriminant for #ty_ident #ty_generics #where_clause {
            fn discriminant(&self) -> i32 {
                unsafe { *<*const _>::from(self).cast::<i32>() }
            }
        }

        impl #impl_generics WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: Writer<F>>(&self, w: W) -> Result {
                let d = <Self as XDRDiscriminant>::discriminant(self);

                match self {
                    #write_variants
                }
                Ok(())
            }
        }
    }
}

fn write_variant(_index: usize, v: &syn::Variant, input: &syn::DeriveInput) -> TokenStream {
    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let n_fields = v.fields.len();
    let var_bindings = var_bindings(&v.fields);
    let write_bindings = token_reduce(&v.fields, write_binding);

    quote! {
        #ty_ident::#var_ident #var_bindings => {
            let mut e = w.write_enum(d, #n_fields)?;

            #write_bindings
        }
    }
}

fn write_binding(index: usize, _field: &syn::Field) -> TokenStream {
    let bind = tmp_binding(index);

    quote! {
        e.write_field(#bind)?;
    }
}

fn var_bindings(fields: &syn::Fields) -> TokenStream {
    match fields {
        syn::Fields::Named(fields) => named_field_bindings(fields),
        syn::Fields::Unnamed(fields) => unnamed_field_bindings(fields),
        syn::Fields::Unit => quote! {},
    }
}

fn unnamed_field_bindings(fields: &syn::FieldsUnnamed) -> TokenStream {
    let bindings = token_reduce(&fields.unnamed, unnamed_field_binding);
    quote! {
        ( #bindings )
    }
}

fn unnamed_field_binding(index: usize, _field: &syn::Field) -> TokenStream {
    let bind = tmp_binding(index);
    quote! { #bind, }
}

fn named_field_bindings(fields: &syn::FieldsNamed) -> TokenStream {
    let bindings = token_reduce(&fields.named, named_field_binding);
    quote! {
        { #bindings }
    }
}

fn named_field_binding(index: usize, field: &syn::Field) -> TokenStream {
    let bind = tmp_binding(index);
    let ident = field.ident.as_ref().unwrap();
    quote! { #ident: #bind, }
}

// ---- derive ReadXDR ------------------------------------------------------

pub fn read_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let mut impls = TokenStream::new();

    impls.extend(read_data(input));

    Ok(quote! {
        const _: () = {
            use core::mem::MaybeUninit as MU;
            use core::mem::transmute;
            use ::xdrs::read_xdr::{ReadXDR, read_xdr};
            use ::xdrs::serialize::reader::{Reader, Error, Result};
            use ::xdrs::discrim::XDRDiscriminant;

            #impls
        };
    })
}

fn read_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(item) => Ok(read_struct(input, item)),
        syn::Data::Enum(item) => Ok(read_enum(input, item)),
        _ => err(input, "Can't derive for this item"),
    }
}

fn read_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> TokenStream {
    let ty_ident = &input.ident;

    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let generics = modify_for_reader_impl(input.generics.clone());
    let (impl_generics, _, _) = generics.split_for_impl();

    let read_fields = token_reduce(&item.fields, read_field);

    quote! {
        impl #impl_generics ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: Reader<'_r>>(r: &mut R) -> Result<Self> {
                let mut value: MU<Self> = MU::uninit();
                let struct_ptr = value.as_mut_ptr();

                #read_fields

                // SAFETY: we have initialized all fields
                let value = unsafe { value.assume_init() };

                Ok(value)
            }
        }
    }
}

fn read_field(index: usize, field: &syn::Field) -> TokenStream {
    let fa = FieldAccess::new(index, field);
    let attrs = FieldAttrs::from_field(field).unwrap();

    let read_field_value = if let Some(as_ty) = attrs.as_type {
        quote! {
            let as_value: #as_ty = read_xdr(r)?;
            let field_value = as_value.into();
        }
    } else {
        quote! {
            let field_value = read_xdr(r)?;
        }
    };

    quote! {
        #read_field_value
        unsafe { (*struct_ptr).#fa = field_value; }
    }
}

fn read_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> TokenStream {
    let ty_ident = &input.ident;
    let n_variants = item.variants.iter().count();
    let n_explicit = item.variants.iter().filter(|v| v.discriminant.is_some()).count();

    let mut default_ix = None;
    if n_variants > 1 && n_explicit == n_variants - 1 {
        // We'll treat the single implicit variant as a default,
        default_ix = Some(item.variants.iter().enumerate()
            .find(|(_, v)| v.discriminant.is_none())
            .map(|(i, _)| i).unwrap());
    } else if !(n_explicit == 0 || n_explicit == n_variants) {
        return syn::Error::new(
            input.span(),
            "derive ReadXDR: Enums must have zero, one, or all implicit variants",
        )
        .into_compile_error();
        // (But we could support other mixes with a little more effort)
    }
    let catch_bad_index = if default_ix.is_none() {
        quote! { _ => Err(Error::BadDiscriminator) }
    } else {
        quote! {}
    };

    let read_variants = token_reduce(&item.variants, |i, v| read_variant(i, v, input, default_ix));

    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let generics = modify_for_reader_impl(input.generics.clone());
    let (impl_generics, _, _) = generics.split_for_impl();

    quote! {
        impl #impl_generics ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: Reader<'_r>>(r: &mut R) -> Result<Self> {
                let d = r.read_i32()?;

                match d {
                    #read_variants
                    #catch_bad_index
                }
            }
        }
    }
}

fn read_variant(index_u: usize, variant: &syn::Variant, input: &syn::DeriveInput, default_ix: Option<usize>) -> TokenStream {
    let index = index_u as i32;
    let var_match = if default_ix == Some(index_u) {
        quote! { _ }
    } else if let Some((_, d)) = variant.discriminant.as_ref() {
        // this formulation is needed in case of an explicit discriminant like "Enum::Variant as i32"
        quote! { d if d == (#d) }
    } else {
        quote! { #index }
    };
    let ty_ident = &input.ident;
    let var_ident = &variant.ident;
    let declare_fields = token_reduce(&variant.fields, declare_uninit_field);
    let read_fields = token_reduce(&variant.fields, |i, _| {
        let bind = tmp_binding(i);
        quote! { #bind.write(read_xdr(r)?); }
    });
    let init_fields = init_fields(&variant.fields);

    quote! {
        #var_match => {
            #declare_fields
            #read_fields
            let value = #ty_ident::#var_ident #init_fields;

            Ok(value)
        }
    }
}

fn declare_uninit_field(index: usize, field: &syn::Field) -> TokenStream {
    let bind = tmp_binding(index);
    let ty = &field.ty;

    quote! {
        let mut #bind: MU<#ty> = MU::uninit();
    }
}

fn init_fields(fields: &syn::Fields) -> TokenStream {
    let init_fields = token_reduce(fields, init_field);
    match fields {
        syn::Fields::Named(_) => quote! { { #init_fields } },
        syn::Fields::Unnamed(_) => quote! { ( #init_fields ) },
        syn::Fields::Unit => quote! {},
    }
}

fn init_field(index: usize, field: &syn::Field) -> TokenStream {
    let bind = tmp_binding(index);

    // SAFETY: all bindings have been written in #read_fields
    if let Some(ident) = field.ident.as_ref() {
        quote! { #ident: unsafe { #bind.assume_init() }, }
    } else {
        quote! { unsafe { #bind.assume_init() }, }
    }
}
