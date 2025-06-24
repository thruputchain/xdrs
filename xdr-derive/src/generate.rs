use crate::util::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

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
        _ => err(input, "Can't derive for this item"),
    }
}

fn write_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> TokenStream {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut fields_write_xdr = TokenStream::new();
    for (i, f) in item.fields.iter().enumerate() {
        fields_write_xdr.extend(write_field(i, f));
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

fn write_field(index: usize, f: &syn::Field) -> TokenStream {
    let fa = FieldAccess::from((f, index));

    quote! {
        s.write_field(&(self.#fa))?;
    }
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
        _ => err(input, "Can't derive for this item"),
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

    let mut lifetime_param: syn::LifetimeParam = syn::parse_quote! { '_r };
    for lt in ty_lifetimes {
        // Add each of the item's lifetime params as a bound on the reader lifetime
        lifetime_param.bounds.push(lt.clone())
    }

    let mut generics = input.generics.clone();
    generics
        .params
        .push(syn::GenericParam::Lifetime(lifetime_param));
    let (impl_generics, _ty_generics, _where_clause) = generics.split_for_impl();

    let mut n_fields: u32 = 0;
    let mut fields_read_xdr = TokenStream::new();
    for (i, f) in item.fields.iter().enumerate() {
        fields_read_xdr.extend(read_field(f, i as u32));
        n_fields += 1;
    }

    quote! {
        impl #impl_generics ReadXDR<'_r> for #ty_ident #ty_generics #where_clause {
            fn read_xdr<R: Reader<'_r>>(r: &mut R) -> Result<Self> {
                let mut value: MU<Self> = MU::uninit();
                let struct_ptr = value.as_mut_ptr();

                let visit_field = |index: u32, r: &mut R| -> ReadResult<()> {
                    match index {
                        #fields_read_xdr
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

fn read_field(f: &syn::Field, index: u32) -> TokenStream {
    let fa = FieldAccess::from((f, index as usize));

    quote! {
                        #index => {
                            let field_value = read_xdr(r)?;
                            unsafe { (*struct_ptr).#fa = field_value; }
                            Ok(())
                        },
    }
}
