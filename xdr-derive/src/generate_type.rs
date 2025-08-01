use crate::util::*;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::Result;

/*
use darling::{self, FromDeriveInput};
#[derive(Debug, FromDeriveInput)]
#[darling(attributes(repr))]
struct Repr {
    #[darling(rename = "i32")]
    pub _i32: Option<()>
}
*/

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
    let declare_fields = token_reduce(&item.fields, declare_struct_field);

    Ok(quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                __xdrs::struct_def(#ty_name, vec![
                    #declare_fields
                ])
            }
        }
    })
}

fn from_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_attrs(input)?;

    let default_ix = find_default_variant(input, item)?;
    let cfg = EnumCfg { default_ix, .. EnumCfg::default() };

    if enum_is_fieldless(item) {
        from_enum_fieldless(input, item, &cfg)
    } else {
        from_enum_with_fields(input, item, &cfg)
    }
}

#[derive(Debug, Default, Clone)]
struct EnumCfg {
    default_ix: Option<usize>,
    enum_name: Option<String>,
}

fn from_enum_with_fields(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    cfg: &EnumCfg
) -> Result<TokenStream> {
    // The enum will have an XDR spec with two definitions:
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

/*
union EnumWithDefault switch (EnumWithDefaultType which) {
	case Alpha:
		void;
	case Beta:
		void;
	default:
		i32 which;
};
*/
fn union_def(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    cfg: &EnumCfg
) -> Result<TokenStream> {
    let ty_name = input.ident.to_string();

    Ok(quote! {
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

    let declare_variants = token_reduce(&item.variants, |i, v| {
        declare_enum_variant(i, v, input, cfg)
    });

    Ok(quote! {
            __xdrs::enum_def(#enum_name, vec![
                #declare_variants
            ])
    })
}

fn declare_struct_field(_index: usize, field: &syn::Field) -> TokenStream {
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
    cfg: &EnumCfg,
) -> TokenStream {
    if Some(index) == cfg.default_ix {
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

fn check_enum_attrs(input: &syn::DeriveInput) -> Result<()> {
    if let Some(repr_attr) = input.attrs.iter().find(|a|
        a.path().get_ident().map(|i| &i.to_string() == "repr").unwrap_or(false))
    {
        if let Ok(repr_ident) = repr_attr.parse_args::<syn::Ident>() {
            let repr_str = repr_ident.to_string();
            if &repr_str != "i32" {
                return err(repr_attr, format!("enum must use #[repr(i32)] to derive XDR; found {repr_str}"));
            }
        } else {
            return err(input, "enum must use #[repr(i32)] to derive XDR; couldn't parse repr args");
        }
    } else {
        return err(input, "enum must use #[repr(i32)] to derive XDR; no #[repr(_)] found");
    }
    Ok(())
}
