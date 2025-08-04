use crate::util::*;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::Result;

pub fn xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let imp = from_input(input)?;

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

fn from_input(input: &syn::DeriveInput) -> Result<TokenStream> {
    let data = &input.data;
    match data {
        syn::Data::Struct(item) => from_struct(input, item),
        syn::Data::Enum(item) => from_enum(input, item),
        _ => err(input, "Can't derive XDR for this item"),
    }
}

fn from_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let ty_name = ty_ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let declare_fields = item.fields.iter().map(declare_struct_field);
    let struct_typedef = quote! {
        __xdrs::struct_def(#ty_name, vec![
            #(#declare_fields)*
        ])
    };

    Ok(quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                #struct_typedef
            }
        }
    })
}

#[derive(Debug, Default, Clone)]
struct EnumCfg {
    enum_name: Option<String>,
}

fn from_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_attrs(input)?;
    check_variants(input, item)?;

    let cfg = EnumCfg::default();

    if enum_is_fieldless(item) {
        from_enum_fieldless(input, item, &cfg)
    } else {
        from_enum_with_fields(input, item, &cfg)
    }
}

fn from_enum_with_fields(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    cfg: &EnumCfg
) -> Result<TokenStream> {
    // The enum will produce two definitions:
    // - an XDR enumeration with the discriminant values
    // - an XDR union with the variant fields

    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let enum_name = Some(input.ident.to_string() + "Type");
    let enum_def = enum_def(input, item, &EnumCfg { enum_name, .. *cfg })?;

    let union_def = union_def(input, item, cfg)?;

    Ok(quote! {
        impl #impl_generics __xdrs::HasSpecification for #ty_ident #ty_generics #where_clause {
            fn spec() -> __xdrs::Specification {
                __xdrs::Specification {
                    definitions: vec![#enum_def, #union_def]
                }
            }
        }
    })
}

fn from_enum_fieldless(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    cfg: &EnumCfg
) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let enum_typedef = enum_typedef(input, item, cfg)?;

    Ok(quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                #enum_typedef
            }
        }
    })
}

fn enum_def(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    cfg: &EnumCfg
) -> Result<TokenStream> {
    let typedef = enum_typedef(input, item, cfg)?;

    Ok(quote! {
        __xdrs::Definition::Type(#typedef)
    })
}

fn enum_typedef(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    cfg: &EnumCfg
) -> Result<TokenStream> {
    // The XDR enum has the same name as the rust type, unless the config overrides that
    let enum_name = cfg.enum_name.clone().unwrap_or_else(|| input.ident.to_string());

    let declare_variants = item.variants.iter().enumerate().map(|(i, v)|
        declare_enum_variant(i, v, input, cfg));

    Ok(quote! {
            __xdrs::enum_def(#enum_name, vec![
                #(#declare_variants)*
            ])
    })
}

fn union_def(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    cfg: &EnumCfg
) -> Result<TokenStream> {
    let ty_name = input.ident.to_string();
    let discrim_type_name = cfg.enum_name.clone()
        .unwrap_or_else(|| ty_name.clone() + "Type");
    let discrim_type = quote! { __xdrs::named_type(#discrim_type_name) };
    let discrim_name = "which";
    let specs = item.variants.iter().filter_map(variant_case_spec);
    let case_specs = quote! { vec![#(#specs),*] };
    let default_variant = find_default_variant(item);
    let default_decl = option_tok(default_variant.map(|_v|
        quote! {
            __xdrs::declare_void()
        }));

    let union_typedef = quote! {
        __xdrs::union_def(
            #ty_name,
            #discrim_name,
            #discrim_type,
            #case_specs,
            #default_decl,
            )
    };

    Ok(quote! {
        __xdrs::Definition::Type(#union_typedef)
    })
}

fn variant_case_spec(variant: &syn::Variant) -> Option<TokenStream> {
    if variant_is_default(variant) {
        return None;
    }

    let variant_name = variant.ident.to_string();
    let decl = variant_fields_decl(variant);

    Some(quote! {
        __xdrs::single_case_spec(#variant_name, #decl)
    })
}

fn variant_fields_decl(variant: &syn::Variant) -> TokenStream {
    let fields = &variant.fields;
    let variant_name = variant.ident.to_string().to_lowercase();

    match fields {
        syn::Fields::Unit => {
            quote! { __xdrs::Declaration::Void }
        }
        syn::Fields::Named(fields_named) => {
            let mut decls = fields_named.named.iter().map(|field| {
                let field_ty = &field.ty;
                let field_name = field.ident.as_ref().unwrap().to_string();
                quote! { <#field_ty as __xdrs::Declare>::declare(ident(#field_name)) }
            });
            if decls.len() == 1 {
                decls.next().unwrap()
            } else {
                // A variant with multiple named fields gets a struct
                // declaration containing them.
                quote! {
                    __xdrs::declare_type(#variant_name,
                        __xdrs::struct_type(vec![ #(#decls),* ]))
                }
            }
        }
        syn::Fields::Unnamed(_) => {
            unimplemented!();
        }
    }
}

fn declare_struct_field(field: &syn::Field) -> TokenStream {
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

// Generate a variant declaration for an XDR enumeration
fn declare_enum_variant(
    index: usize,
    variant: &syn::Variant,
    _input: &syn::DeriveInput,
    _cfg: &EnumCfg,
) -> TokenStream {
    if variant_is_default(variant) {
        // Ignore default arm
        return quote!{}
    }

    let ident = &variant.ident.to_string();

    let index = index as i32;
    let index_expr = quote! { #index };
    let discriminant_expr = variant.discriminant.as_ref().map(|(_, expr)|
        expr.to_token_stream());
    let discriminant = discriminant_expr.as_ref().unwrap_or(&index_expr);

    quote! {
        (ident(#ident), signed(#discriminant)),
    }
}
