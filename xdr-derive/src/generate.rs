use crate::attrs::*;
use crate::util::*;
use darling::*;
use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::Result;
use syn::visit::{self, Visit};

// ---- derive WriteXDR ------------------------------------------------------

pub fn write_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let attrs = InputAttrs::from_derive_input(input)?;
    attrs.check(input)?;
    let krate = attrs.crate_path();
    let impls = write_data(input, &attrs)?;

    Ok(quote! {
        const _: () = {
            mod __xdrs {
                pub use #krate::serialize::writer::{Writer, WriteResult};
                pub use #krate::discrim::XDRDiscriminant;
                pub use #krate::WriteXDR;
            }

            #impls
        };
    })
}

fn write_data(input: &syn::DeriveInput, attrs: &InputAttrs) -> Result<TokenStream> {
    if let Some(into_type) = attrs.into_type.as_ref().or(attrs.via.as_ref()) {
        return Ok(write_into_type(input, into_type));
    }

    match &input.data {
        syn::Data::Struct(item) => write_struct(input, item),
        syn::Data::Enum(item) => write_enum(input, item),
        _ => err(input, "Can't derive for this item"),
    }
}

fn write_into_type(input: &syn::DeriveInput, into_type: &syn::Type) -> TokenStream {
    let ty_ident = &input.ident;

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, w: &mut W) -> __xdrs::WriteResult<()> {
                <#into_type as __xdrs::WriteXDR>::write_xdr(
                    &::core::convert::Into::into(::core::clone::Clone::clone(self)),
                    w,
                )
            }
        }
    }
}

fn write_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| write_field(i, f))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, w: &mut W) -> __xdrs::WriteResult<()> {
                #(#write_fields)*
                Ok(())
            }
        }
    })
}

fn write_field(index: usize, field: &syn::Field) -> Result<TokenStream> {
    let ty = &field.ty;
    let fa = FieldAccess::new(index, field);
    let attrs = FieldAttrs::from_field(field)?;
    attrs.check(field)?;

    if let Some(into_type) = attrs.into_type.as_ref().or(attrs.via.as_ref()) {
        Ok(quote! {
            <#into_type as __xdrs::WriteXDR>::write_xdr(
                &::core::convert::Into::into(::core::clone::Clone::clone(&self.#fa)),
                w,
            )?;
        })
    } else {
        Ok(quote! {
            <#ty as __xdrs::WriteXDR>::write_xdr(&self.#fa, w)?;
        })
    }
}

fn write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item, has_xdr_discrim_type(input))?;

    if let Some(toks) = write_enum_fast(input, item) {
        return Ok(toks);
    }

    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_variants: Vec<_> = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| write_variant(i, v, input))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl #impl_generics __xdrs::WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<F, W: __xdrs::Writer<F>>(&self, __write_xdr_writer: &mut W) -> __xdrs::WriteResult<()> {
                match self {
                    #(#write_variants)*
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

fn write_variant(index: usize, v: &syn::Variant, input: &syn::DeriveInput) -> Result<TokenStream> {
    let is_default_arm = variant_is_default(v);

    let var_ident = &v.ident;
    let ty_ident = &input.ident;

    let var_bindings = var_bindings(&v.fields);
    let write_bindings: Vec<_> = v
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            if is_default_arm && i == 0 {
                Ok(quote! {})
            } else {
                write_binding(i, field)
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        // An explicit-discriminant variant should never be considered a default arm
        assert!(!is_default_arm);

        quote! { #d }
    } else if is_default_arm {
        // Use the default-arm value as the discriminant
        quote! { *_self_0 }
    } else {
        // Use the zero-based index
        quote! { #variant_idx }
    };

    Ok(quote! {
        #ty_ident::#var_ident #var_bindings => {
            __write_xdr_writer.write_i32(#discriminant)?;

            #(#write_bindings)*
        }
    })
}

fn write_binding(index: usize, field: &syn::Field) -> Result<TokenStream> {
    let ty = &field.ty;
    let bind = tmp_binding(index);
    let attrs = FieldAttrs::from_field(field)?;
    attrs.check(field)?;

    if let Some(into_type) = attrs.into_type.as_ref().or(attrs.via.as_ref()) {
        Ok(quote! {
            <#into_type as __xdrs::WriteXDR>::write_xdr(
                &::core::convert::Into::into(::core::clone::Clone::clone(#bind)),
                __write_xdr_writer,
            )?;
        })
    } else {
        Ok(quote! {
            <#ty as __xdrs::WriteXDR>::write_xdr(#bind, __write_xdr_writer)?;
        })
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
    let attrs = InputAttrs::from_derive_input(input)?;
    attrs.check(input)?;
    let krate = attrs.crate_path();
    let impls = read_data(input, &attrs)?;

    Ok(quote! {
        const _: () = {
            mod __xdrs {
                pub use #krate::discrim::XDRDiscriminant;
                pub use #krate::read_xdr::{ReadXDR, read_xdr};
                pub use #krate::serialize::reader::{Reader, ReadError, ReadResult};
            }

            #impls
        };
    })
}

fn read_data(input: &syn::DeriveInput, attrs: &InputAttrs) -> Result<TokenStream> {
    if let Some(from_type) = attrs.from_type.as_ref().or(attrs.via.as_ref()) {
        return read_from_type(input, from_type);
    }

    match &input.data {
        syn::Data::Struct(item) => read_struct(input, item),
        syn::Data::Enum(item) => read_enum(input, item),
        _ => err(input, "Can't derive for this item"),
    }
}

// Target type must impl From<from_type>
fn read_from_type(input: &syn::DeriveInput, from_type: &syn::Type) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let mut generics = modify_for_reader_impl(input.generics.clone());
    add_read_bound_if_lifetime_dependent(&mut generics, from_type, input);
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics __xdrs::ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: __xdrs::Reader<'_r>>(r: &mut R) -> __xdrs::ReadResult<Self> {
                let from_val: #from_type = __xdrs::read_xdr(r)?;
                Ok(::core::convert::Into::into(from_val))
            }
        }
    })
}

fn read_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    let ty_ident = &input.ident;

    let (_, ty_generics, _) = input.generics.split_for_impl();
    let mut generics = modify_for_reader_impl(input.generics.clone());
    add_read_bounds_for_struct_fields(&mut generics, input, &item.fields)?;
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    let read_fields = read_fields(&item.fields)?;

    Ok(quote! {
        impl #impl_generics __xdrs::ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: __xdrs::Reader<'_r>>(r: &mut R) -> __xdrs::ReadResult<Self> {
                Ok(#ty_ident #read_fields)
            }
        }
    })
}

fn read_fields(fields: &syn::Fields) -> Result<TokenStream> {
    let read_fields = fields
        .iter()
        .enumerate()
        .map(|(i, f)| read_field(i, f))
        .collect::<Result<Vec<_>>>()?;

    match fields {
        syn::Fields::Named(_) => Ok(quote! { { #(#read_fields),* } }),
        syn::Fields::Unnamed(_) => Ok(quote! { ( #(#read_fields),* ) }),
        syn::Fields::Unit => Ok(quote! {}),
    }
}

fn read_field(_index: usize, field: &syn::Field) -> Result<TokenStream> {
    let attrs = FieldAttrs::from_field(field)?;
    attrs.check(field)?;

    let field_spec = if let Some(field_ident) = field.ident.as_ref() {
        quote! { #field_ident: }
    } else {
        quote! {}
    };

    let field_value = if let Some(from_type) = attrs.from_type.as_ref().or(attrs.via.as_ref()) {
        quote! { {
            let from_value: #from_type = __xdrs::read_xdr(r)?;
            ::core::convert::Into::into(from_value)
        } }
    } else {
        quote! {
            __xdrs::read_xdr(r)?
        }
    };

    Ok(quote! { #field_spec #field_value })
}

fn add_read_bound(generics: &mut syn::Generics, ty: &syn::Type) {
    let where_clause = generics.make_where_clause();
    where_clause
        .predicates
        .push(syn::parse_quote! { #ty: __xdrs::ReadXDR<'_r> });
}

fn add_read_bound_if_lifetime_dependent(
    generics: &mut syn::Generics,
    ty: &syn::Type,
    input: &syn::DeriveInput,
) {
    let lifetimes = generic_lifetime_idents(&input.generics);
    if !lifetimes.is_empty()
        && type_contains_lifetime(ty, &lifetimes)
        && !type_mentions_ident(ty, &input.ident)
    {
        add_read_bound(generics, ty);
    }
}

fn add_read_bounds_for_struct_fields(
    generics: &mut syn::Generics,
    input: &syn::DeriveInput,
    fields: &syn::Fields,
) -> Result<()> {
    for field in fields {
        let attrs = FieldAttrs::from_field(field)?;
        attrs.check(field)?;

        if let Some(from_type) = attrs.from_type.as_ref().or(attrs.via.as_ref()) {
            add_read_bound_if_lifetime_dependent(generics, from_type, input);
        } else {
            add_read_bound_if_lifetime_dependent(generics, &field.ty, input);
        }
    }

    Ok(())
}

fn add_read_bounds_for_enum_variants(
    generics: &mut syn::Generics,
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
) {
    for variant in &item.variants {
        let is_default = variant_is_default(variant);

        for (index, field) in variant.fields.iter().enumerate() {
            if is_default && index == 0 {
                continue;
            }

            let attrs = FieldAttrs::from_field(field).ok();
            let from_type = attrs
                .as_ref()
                .and_then(|a| a.from_type.as_ref().or(a.via.as_ref()));
            if let Some(from_type) = from_type {
                add_read_bound_if_lifetime_dependent(generics, from_type, input);
            } else {
                add_read_bound_if_lifetime_dependent(generics, &field.ty, input);
            }
        }
    }
}

fn generic_lifetime_idents(generics: &syn::Generics) -> Vec<Ident> {
    generics
        .lifetimes()
        .map(|param| param.lifetime.ident.clone())
        .collect()
}

fn lifetime_matches_any(lifetime: &syn::Lifetime, lifetimes: &[Ident]) -> bool {
    lifetimes.contains(&lifetime.ident)
}

struct LifetimeUse<'a> {
    lifetimes: &'a [Ident],
    found: bool,
}

impl<'ast> Visit<'ast> for LifetimeUse<'_> {
    fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
        if lifetime_matches_any(lifetime, self.lifetimes) {
            self.found = true;
        }
    }
}

fn type_contains_lifetime(ty: &syn::Type, lifetimes: &[Ident]) -> bool {
    let mut visitor = LifetimeUse {
        lifetimes,
        found: false,
    };
    visitor.visit_type(ty);
    visitor.found
}

struct IdentUse<'a> {
    ident: &'a Ident,
    found: bool,
}

impl<'ast> Visit<'ast> for IdentUse<'_> {
    fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
        if segment.ident == *self.ident {
            self.found = true;
        }

        visit::visit_path_segment(self, segment);
    }
}

fn type_mentions_ident(ty: &syn::Type, ident: &Ident) -> bool {
    let mut visitor = IdentUse {
        ident,
        found: false,
    };
    visitor.visit_type(ty);
    visitor.found
}

fn read_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item, has_xdr_discrim_type(input))?;
    let default_variant = find_default_variant(item);

    let ty_ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let mut generics = modify_for_reader_impl(input.generics.clone());
    add_read_bounds_for_enum_variants(&mut generics, input, item);
    let (impl_generics, _, where_clause) = generics.split_for_impl();

    let read_variants: Vec<_> = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| read_variant(i, v, input))
        .collect::<Result<Vec<_>>>()?;

    let catch_bad_index = if default_variant.is_none() {
        quote! { _ => Err(__xdrs::ReadError::BadDiscriminator(d)) }
    } else {
        quote! {}
    };

    Ok(quote! {
        impl #impl_generics __xdrs::ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: __xdrs::Reader<'_r>>(r: &mut R) -> __xdrs::ReadResult<Self> {
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
) -> Result<TokenStream> {
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
    let read_fields: Vec<_> = variant
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            if is_default && i == 0 {
                return Ok(quote! {});
            }

            let attrs = FieldAttrs::from_field(field)?;
            attrs.check(field)?;
            let bind = tmp_binding(i);

            if let Some(from_type) = attrs.from_type.as_ref().or(attrs.via.as_ref()) {
                let ty = &field.ty;
                Ok(quote! {
                    let #bind: #from_type = __xdrs::read_xdr(r)?;
                    let #bind: #ty = <_ as ::core::convert::Into<_>>::into(#bind);
                })
            } else {
                let ty = &field.ty;
                Ok(quote! { let #bind: #ty = __xdrs::read_xdr(r)?; })
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let init_fields = init_fields(&variant.fields, is_default);

    Ok(quote! {
        #var_match => {
            #(#read_fields)*
            Ok(#ty_ident::#var_ident #init_fields)
        }
    })
}

fn init_fields(fields: &syn::Fields, is_default: bool) -> TokenStream {
    let init_fields = token_reduce(fields, |i, field| {
        let bind = tmp_binding(i);

        let init_val = if is_default && i == 0 {
            // Field 0 is the discriminant
            quote! { d }
        } else {
            quote! { #bind }
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
