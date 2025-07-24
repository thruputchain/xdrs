use crate::util::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

use darling::FromField;

#[derive(Debug, FromField)]
#[darling(attributes(xdrs))]
struct FieldAttrs {
    pub as_type: Option<syn::Type>,
}

// ---- derive WriteXDR ------------------------------------------------------

pub fn write_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let mut impls = TokenStream::new();

    impls.extend(write_data(input));

    Ok(quote! {
        const _: () = {
            mod __xdrs {
                pub use ::xdrs::serialize::writer::{Writer, Result};
                pub use ::xdrs::discrim::XDRDiscriminant;
                pub use ::xdrs::WriteXDR;
            }

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
    let write_fields = token_reduce(&item.fields, write_field);
    quote! {
        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, w: &mut W) -> __xdrs::Result {
                #write_fields
                Ok(())
            }
        }
    }
}

fn write_field(index: usize, field: &syn::Field) -> TokenStream {
    let ty = &field.ty;
    let fa = FieldAccess::new(index, field);
    let attrs = FieldAttrs::from_field(field).unwrap();

    if let Some(as_ty) = attrs.as_type {
        quote! {
            let v: &#as_ty = ::core::ops::Deref::deref(&self.#fa);
            <#as_ty as __xdrs::WriteXDR>::write_xdr(v, w)?;
        }
    } else {
        quote! {
            let v: &#ty = &(self.#fa);
            <#ty as __xdrs::WriteXDR>::write_xdr(v, w)?;
        }
    }
}

fn write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> TokenStream {
    if let Some(toks) = write_enum_fast(input, item) {
        return toks;
    }

    let default_ix = match find_default_variant(input, item) {
        Ok(d) => d,
        Err(e) => {
            return e;
        }
    };

    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_variants = token_reduce(&item.variants, |i, v| {
        write_variant(i, v, input, default_ix)
    });

    quote! {
        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, __write_xdr_writer: &mut W) -> __xdrs::Result {
                match self {
                    #write_variants
                }
                Ok(())
            }
        }
    }
}

// Give an alternate implementation in certain cases
fn write_enum_fast(_input: &syn::DeriveInput, _item: &syn::DataEnum) -> Option<TokenStream> {
    // TODO: Determine when, if ever, to use "fast" enums.
    // (see https://doc.rust-lang.org/reference/items/enumerations.html)
    // (see https://github.com/rust-lang/rfcs/blob/master/text/2195-really-tagged-unions.md)

    None

    /*
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_variants = token_reduce(&item.variants, |i, v| write_variant(i, v, input));

    Some(quote! {
        impl #impl_generics __xdrs::XDRDiscriminant for #ty_ident #ty_generics {
            fn discriminant(&self) -> i32 {
                // or use numeric cast if enum is unit-only, or in some other cases
                unsafe { *<*const _>::from(self).cast::<i32>() }
            }
        }

        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, __write_xdr_writer: &mut W) -> __xdrs::Result {
                let d = <Self as __xdrs::XDRDiscriminant>::discriminant(self);
                __write_xdr_writer.write_i32(d)
            }
        }
    })
    */
}

fn write_variant(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    default_ix: Option<usize>,
) -> TokenStream {
    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let var_bindings = var_bindings(&v.fields);
    let write_bindings = if Some(index) == default_ix {
        // only write the discriminant
        quote! {}
    } else {
        token_reduce(&v.fields, write_binding)
    };
    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        if let Some(default_ix_inner) = default_ix {
            assert_ne!(default_ix_inner, index);
        }
        quote! { #d }
    } else {
        if let Some(default_ix_inner) = default_ix {
            if index == default_ix_inner {
                quote! { *_self_0 }
            } else {
                quote! { #variant_idx }
            }
        } else {
            quote! { #variant_idx }
        }
    };

    quote! {
        #ty_ident::#var_ident #var_bindings => {
            __write_xdr_writer.write_i32(#discriminant)?;

            #write_bindings
        }
    }
}

fn write_binding(index: usize, field: &syn::Field) -> TokenStream {
    let ty = &field.ty;
    let bind = tmp_binding(index);

    quote! {
        <#ty as __xdrs::WriteXDR>::write_xdr(#bind, __write_xdr_writer)?;
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
            mod __xdrs {
                pub use core::mem::MaybeUninit as MU;
                pub use ::xdrs::discrim::XDRDiscriminant;
                pub use ::xdrs::read_xdr::{ReadXDR, read_xdr};
                pub use ::xdrs::serialize::reader::{Reader, Error, Result};
            }

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
        impl #impl_generics __xdrs::ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: __xdrs::Reader<'_r>>(r: &mut R) -> __xdrs::Result<Self> {
                let mut value: __xdrs::MU<Self> = __xdrs::MU::uninit();
                let struct_ptr = value.as_mut_ptr();

                // TODO: cleanup on error to avoid memory leaks
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
            let as_value: #as_ty = __xdrs::read_xdr(r)?;
            let field_value = as_value.into();
        }
    } else {
        quote! {
            let field_value = __xdrs::read_xdr(r)?;
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
    let n_explicit = item
        .variants
        .iter()
        .filter(|v| v.discriminant.is_some())
        .count();
    let n_implicit = n_variants - n_explicit;

    let default_ix = match find_default_variant(input, item) {
        Ok(d) => d,
        Err(e) => {
            return e;
        }
    };

    if default_ix.is_none() && !(n_explicit == 0 || n_implicit == 0) {
        return err_tok(
            input,
            "derive ReadXDR: Enums discriminants must be all explicit or all \
             implicit (except there may be one variant called \"Default\" \
             with a single i32 field)",
        );
        // (But we could support other mixes with a little more effort)
    }

    let catch_bad_index = if default_ix.is_none() {
        quote! { _ => Err(__xdrs::Error::BadDiscriminator(d)) }
    } else {
        quote! {}
    };

    let read_variants = token_reduce(&item.variants, |i, v| read_variant(i, v, input, default_ix));

    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let generics = modify_for_reader_impl(input.generics.clone());
    let (impl_generics, _, _) = generics.split_for_impl();

    quote! {
        impl #impl_generics __xdrs::ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: __xdrs::Reader<'_r>>(r: &mut R) -> __xdrs::Result<Self> {
                let d = r.read_i32()?;

                match d {
                    #read_variants
                    #catch_bad_index
                }
            }
        }
    }
}

fn read_variant(
    index_u: usize,
    variant: &syn::Variant,
    input: &syn::DeriveInput,
    default_ix: Option<usize>,
) -> TokenStream {
    let index = index_u as i32;
    let is_default = Some(index_u) == default_ix;
    let var_match = if is_default {
        quote! { _ }
    } else if let Some((_, d)) = variant.discriminant.as_ref() {
        // this formulation is needed in case of an explicit discriminant expression like "Enum::Variant as i32"
        quote! { d if d == (#d) }
    } else {
        quote! { #index }
    };
    let ty_ident = &input.ident;
    let var_ident = &variant.ident;
    let declare_fields = token_reduce(&variant.fields, |i, field| {
        if is_default {
            return quote! {};
        }

        let bind = tmp_binding(i);
        let ty = &field.ty;

        quote! {
            let mut #bind: __xdrs::MU<#ty> = __xdrs::MU::uninit();
        }
    });
    let read_fields = token_reduce(&variant.fields, |i, _| {
        if is_default {
            return quote! {};
        }

        let bind = tmp_binding(i);
        quote! { #bind.write(__xdrs::read_xdr(r)?); }
    });
    let init_fields = init_fields(&variant.fields, is_default);

    quote! {
        #var_match => {
            #declare_fields
            #read_fields
            let value = #ty_ident::#var_ident #init_fields;

            Ok(value)
        }
    }
}

fn init_fields(fields: &syn::Fields, is_default: bool) -> TokenStream {
    let init_fields = token_reduce(fields, |i, field| {
        let bind = tmp_binding(i);

        let init_val = if is_default {
            // The value is the discriminant
            quote! { d }
        } else {
            // SAFETY: all bindings have been written in #read_fields
            quote! { unsafe { #bind.assume_init() } }
        };

        if let Some(ident) = field.ident.as_ref() {
            quote! { #ident: #init_val, }
        } else {
            quote! { #init_val, }
        }
    });

    match fields {
        syn::Fields::Named(_) => quote! { { #init_fields } },
        syn::Fields::Unnamed(_) => quote! { ( #init_fields ) },
        syn::Fields::Unit => quote! {},
    }
}

fn find_default_variant(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
) -> core::result::Result<Option<usize>, TokenStream> {
    let variants = &item.variants;
    let n_variants = variants.iter().count();
    let n_explicit = variants.iter().filter(|v| v.discriminant.is_some()).count();
    let n_implicit = n_variants - n_explicit;

    if n_variants > 1 && n_implicit == 1 {
        if let Some(ix) = variants
            .iter()
            .enumerate()
            .find(|(_, v)| {
                v.discriminant.is_none()
                    && v.fields.len() == 1
                    && v.ident.to_string().as_str() == "Default"
            })
            .map(|(i, _)| i)
        {
            Ok(Some(ix))
        } else {
            Err(err_tok(
                input,
                "derive ReadXDR: default variants must have one i32 field and \
                 be called \"Default\"",
            ))
        }
    } else {
        Ok(None)
    }
}
