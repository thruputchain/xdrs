use crate::util::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;
use syn::spanned::Spanned;

pub fn write_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let mut impls = TokenStream::new();

    impls.extend(write_data(input));

    Ok(quote! {
        const _: () = {
            use ::cxs::xdr::writer::Result;

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
            fn write_xdr<W: Writer>(&self, w: W) -> Result {
                let mut s = w.write_struct(#n_fields)?;

                #write_fields

                Ok(())
            }
        }
    }
}

fn write_field(index: usize, field: &syn::Field) -> TokenStream {
    let fa = FieldAccess::new(index, field);

    quote! {
        s.write_field(&(self.#fa))?;
    }
}

fn write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> TokenStream {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let write_variants = token_reduce(&item.variants, |i, v| write_variant(i, v, input));

    quote! {
        impl #impl_generics WriteXDR for #ty_ident #ty_generics #where_clause {
            fn write_xdr<W: Writer>(&self, w: W) -> Result {
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

pub fn read_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let mut impls = TokenStream::new();

    impls.extend(read_data(input));

    Ok(quote! {
        const _: () = {
            use core::mem::MaybeUninit as MU;
            use core::mem::transmute;
            use ::cxs::xdr::reader::Result;

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

    let n_fields = item.fields.len() as u32;
    let read_fields = token_reduce(&item.fields, read_field);

    quote! {
        impl #impl_generics ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: Reader<'_r>>(r: &mut R) -> Result<Self> {
                let mut value: MU<Self> = MU::uninit();
                let struct_ptr = value.as_mut_ptr();

                let visit_field = |index: u32, r: &mut R| -> ReadResult<()> {
                    match index {
                        #read_fields
                        _ => Err(::cxs::ReadError::BadFieldIndex)
                    }
                };

                r.read_struct(#n_fields, visit_field)?;
                // SAFETY: we depend on `read_struct` initializing all fields
                let value = unsafe { value.assume_init() };

                Ok(value)
            }
        }
    }
}

fn read_field(index: usize, field: &syn::Field) -> TokenStream {
    let fa = FieldAccess::new(index, field);
    let index = index as u32;

    quote! {
        #index => {
            let field_value = read_xdr(r)?;
            unsafe { (*struct_ptr).#fa = field_value; }
            Ok(())
        },
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
    if !(n_explicit == 0 || n_explicit == n_variants) {
        return syn::Error::new(
            input.span(),
            "derive ReadXDR: Enums must have all explicit discriminators or all implicit",
        )
        .into_compile_error();
        // (But we could support a mix with a little more effort)
    }

    let read_variants = token_reduce(&item.variants, |i, v| read_variant(i, v, input));

    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let generics = modify_for_reader_impl(input.generics.clone());
    let (impl_generics, _, _) = generics.split_for_impl();

    quote! {
        impl #impl_generics ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: Reader<'_r>>(r: &mut R) -> Result<Self> {
                let d = r.read_i32()?;

                match d {
                    #read_variants
                    _ => Err(::cxs::ReadError::BadFieldIndex)
                }
            }
        }
    }
}

fn read_variant(index: usize, variant: &syn::Variant, input: &syn::DeriveInput) -> TokenStream {
    let index = index as i32;
    let var_match = if let Some((_, d)) = variant.discriminant.as_ref() {
        quote! { #d }
    } else {
        quote! { #index }
    };
    let ty_ident = &input.ident;
    let var_ident = &variant.ident;
    let n_fields = variant.fields.len() as u32;
    let declare_fields = token_reduce(&variant.fields, declare_uninit_field);
    let read_var_fields = token_reduce(&variant.fields, read_var_field);
    let init_var_fields = init_var_fields(&variant.fields);
    let visit_variant_fields = if n_fields > 0 {
        quote! {
            let visit_field = |index: u32, r: &mut R| -> ReadResult<()> {
                match index {
                    #read_var_fields
                    _ => Err(::cxs::ReadError::BadFieldIndex)
                }
            };

            r.read_enum(#n_fields, visit_field)?;
        }
    } else {
        quote! {}
    };

    quote! {
        #var_match => {
            #declare_fields

            #visit_variant_fields

            // SAFETY: we depend on `read_enum` initializing all variant fields
            let value = #ty_ident::#var_ident #init_var_fields;

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

fn read_var_field(index: usize, _field: &syn::Field) -> TokenStream {
    let bind = tmp_binding(index);
    let index = index as u32;

    quote! {
        #index => {
            #bind.write(read_xdr(r)?);
            Ok(())
        },
    }
}

fn init_var_fields(fields: &syn::Fields) -> TokenStream {
    let init_fields = token_reduce(fields, init_field);
    match fields {
        syn::Fields::Named(_) => quote! { { #init_fields } },
        syn::Fields::Unnamed(_) => quote! { ( #init_fields ) },
        syn::Fields::Unit => quote! {},
    }
}

fn init_field(index: usize, field: &syn::Field) -> TokenStream {
    let bind = tmp_binding(index);

    if let Some(ident) = field.ident.as_ref() {
        quote! { #ident: unsafe { #bind.assume_init() }, }
    } else {
        quote! { unsafe { #bind.assume_init() }, }
    }
}

fn tmp_binding(index: usize) -> syn::Ident {
    quote::format_ident!("_self_{}", index)
}

fn modify_for_reader_impl(mut generics: syn::Generics) -> syn::Generics {
    let ty_lifetimes = generics.params.iter().filter_map(|g| {
        if let syn::GenericParam::Lifetime(param) = g {
            Some(&param.lifetime)
        } else {
            None
        }
    });

    let mut lifetime_param: syn::LifetimeParam = syn::parse_quote! { '_r };
    for lt in ty_lifetimes {
        // Add each of the item's lifetime params as a bound on the reader lifetime
        lifetime_param.bounds.push(lt.clone())
    }

    generics
        .params
        .push(syn::GenericParam::Lifetime(lifetime_param));

    generics
}
