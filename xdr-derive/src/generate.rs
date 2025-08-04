use crate::util::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;
use darling::FromField;

// TODO:

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
        syn::Data::Enum(item) => write_enum(input, item),
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

fn write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_attrs(input)?;
    check_variants(input, item)?;

    if let Some(toks) = write_enum_fast(input, item) {
        return Ok(toks);
    }

    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_variants = token_reduce(&item.variants, |i, v| {
        write_variant(i, v, input)
    });

    Ok(quote! {
        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, __write_xdr_writer: &mut W) -> __xdrs::Result {
                match self {
                    #write_variants
                }
                Ok(())
            }
        }
    })
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
) -> TokenStream {
    let is_default_arm = variant_is_default(v);

    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let var_bindings = var_bindings(&v.fields);
    let write_bindings = if is_default_arm {
        // Write only the discriminant
        quote! {}
    } else {
        token_reduce(&v.fields, write_binding)
    };
    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        // An explicit-discriminant variant should never be considered a default arm
        assert!(!is_default_arm);

        quote! { #d }
    } else {
        if is_default_arm {
            // Use the default-arm value as the discriminant
            quote! { *_self_0 }
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
        syn::Data::Enum(item) => read_enum(input, item),
        _ => err(input, "Can't derive for this item"),
    }
}

fn read_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> TokenStream {
    let ty_ident = &input.ident;

    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let generics = modify_for_reader_impl(input.generics.clone());
    let (impl_generics, _, _) = generics.split_for_impl();

    let read_fields = read_fields(&item.fields);

    quote! {
        impl #impl_generics __xdrs::ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: __xdrs::Reader<'_r>>(r: &mut R) -> __xdrs::Result<Self> {
                Ok(#ty_ident #read_fields)
            }
        }
    }
}

fn read_fields(fields: &syn::Fields) -> TokenStream {
    let read_fields = token_reduce(fields, read_field);
    match fields {
        syn::Fields::Named(_) => quote! { { #read_fields } },
        syn::Fields::Unnamed(_) => quote! { ( #read_fields ) },
        syn::Fields::Unit => quote! {},
    }
}

fn read_field(_index: usize, field: &syn::Field) -> TokenStream {
    let attrs = FieldAttrs::from_field(field).unwrap();

    let field_spec = if let Some(field_ident) = field.ident.as_ref() {
        quote! { #field_ident: }
    } else {
        quote! {}
    };

    let field_value = if let Some(as_ty) = attrs.as_type {
        quote! { {
            let as_value: #as_ty = __xdrs::read_xdr(r)?;
            as_value.into()
        } }
    } else {
        quote! {
            __xdrs::read_xdr(r)?
        }
    };

    quote! { #field_spec #field_value, }
}

fn read_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_attrs(input)?;
    check_variants(input, item)?;
    let default_variant = find_default_variant(item);

    let ty_ident = &input.ident;
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let generics = modify_for_reader_impl(input.generics.clone());
    let (impl_generics, _, _) = generics.split_for_impl();

    let read_variants = item.variants.iter().enumerate().map(|(i, v)|
        read_variant(i, v, input));

    let catch_bad_index = if default_variant.is_none() {
        quote! { _ => Err(__xdrs::Error::BadDiscriminator(d)) }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #impl_generics __xdrs::ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: __xdrs::Reader<'_r>>(r: &mut R) -> __xdrs::Result<Self> {
                let d = r.read_i32()?;

                match d {
                    #(#read_variants)*
                    #catch_bad_index
                }
            }
        }
    })
}

fn read_variant(
    index_u: usize,
    variant: &syn::Variant,
    input: &syn::DeriveInput,
) -> TokenStream {
    let index = index_u as i32;
    let is_default = variant_is_default(variant);
    let var_match = if is_default {
        quote! { _ }
    } else if let Some((_, d)) = variant.discriminant.as_ref() {
        // this formulation is needed in case of an explicit discriminant
        // expression like "Enum::Variant as i32"
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

        // TODO: remove this unnecessary MU stuff
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
