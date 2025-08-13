use crate::util::*;
use darling::{FromDeriveInput, FromField};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::Result;

#[derive(Clone, Debug, FromField, FromDeriveInput)]
#[darling(attributes(xdr))]
struct Attrs {
    // The name for the XDR item, if different from the rust name
    name: Option<String>,

    // The name of an existing XDR type to use as the discriminant, instead
    // of generating one
    discrim_type: Option<String>,

    // The name of the discriminant variable, if not "which"
    discrim_var: Option<String>,
}

impl Attrs {
    fn discrim_type_name(&self, rust_enum_name: &str) -> String {
        self.discrim_type
            .clone()
            .map(|t| t.to_string())
            .unwrap_or_else(|| rust_enum_name.to_string() + "Type")
    }

    fn discrim_var_name(&self) -> String {
        self.discrim_var.clone().unwrap_or("which".to_string())
    }
}

/// Generate implementations of HasTypeDef (and for datafull enums, HasSpecification)
pub fn xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let imp = from_input(input)?;

    Ok(quote! {
        const _: () = {
            mod __xdrs {
                pub use ::xdr_repr::ty::*;
                pub use ::xdr_repr::xdr::*;
                pub use ::xdr_repr::construct::*;
                pub use ::std::vec;
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
    if item.fields.is_empty() {
        return Err(error(input, "XDR: can't represent a struct with no fields"));
    }

    let ty_ident = &input.ident;
    let ty_name = ty_ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let declare_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| declare_struct_field(i, field));
    let struct_typedef = quote! {
        __xdrs::struct_def(#ty_name, __xdrs::vec![
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

fn from_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item)?;

    let attrs = Attrs::from_derive_input(input)?;

    if enum_is_fieldless(item) {
        from_enum_fieldless(input, item, &attrs)
    } else {
        from_enum_with_fields(input, item, &attrs)
    }
}

fn from_enum_fieldless(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    attrs: &Attrs,
) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let enum_typedef = enum_typedef(input, item, attrs)?;

    // For a fieldless enum, we define an XDR enumeration
    Ok(quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                #enum_typedef
            }
        }
    })
}

fn from_enum_with_fields(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    attrs: &Attrs,
) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let mut impls = vec![];

    // Generate a union typedef for this fieldful rust enum type
    let union_typedef = union_typedef(input, item, attrs)?;
    impls.push(quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                #union_typedef
            }
        }
    });

    // We'll also implement HasSpecification for the type, as we may require
    // multiple definitions.  It will contain at least the union definition,
    // which, instead of generating again, we can get via the above HasTypeDef impl:
    let mut spec_definitions = vec![quote! { #ty_ident::def() }];

    if attrs.discrim_type.is_none() {
        // No existing discriminant type was provided, so we also define
        // an XDR enumeration to serve as the discriminant

        let rust_enum_name = ty_ident.to_string();
        let attrs = Attrs {
            name: Some(attrs.discrim_type_name(&rust_enum_name)),
            ..attrs.clone()
        };

        spec_definitions.insert(0, enum_def(input, item, &attrs)?);
    };

    impls.push(quote! {
        impl #impl_generics __xdrs::HasSpecification for #ty_ident #ty_generics #where_clause {
            fn spec() -> __xdrs::Specification {
                __xdrs::Specification {
                    definitions: __xdrs::vec![#(#spec_definitions),*]
                }
            }
        }
    });

    Ok(quote! { #(#impls)* })
}

// Make an expression that produces an XDR enumeration definition
fn enum_def(input: &syn::DeriveInput, item: &syn::DataEnum, attrs: &Attrs) -> Result<TokenStream> {
    let typedef = enum_typedef(input, item, attrs)?;

    Ok(quote! {
        __xdrs::Definition::Type(#typedef)
    })
}

// Make an expression that produces an XDR enumeration typedef
fn enum_typedef(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    attrs: &Attrs,
) -> Result<TokenStream> {
    // The XDR enum has the same name as the rust type, unless the attrs override that
    let enum_name = attrs
        .name
        .clone()
        .unwrap_or_else(|| input.ident.to_string());

    let declare_variants = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| declare_enum_variant(i, v));

    Ok(quote! {
        __xdrs::enum_def(
            #enum_name,
            __xdrs::vec![#(#declare_variants)*])
    })
}

// Make an expression that produces an XDR union definition
#[allow(dead_code)]
fn union_def(input: &syn::DeriveInput, item: &syn::DataEnum, attrs: &Attrs) -> Result<TokenStream> {
    let union_typedef = union_typedef(input, item, attrs)?;

    Ok(quote! {
        __xdrs::Definition::Type(#union_typedef)
    })
}

// Make an expression that produces an XDR union typedef
fn union_typedef(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    attrs: &Attrs,
) -> Result<TokenStream> {
    let ty_name = input.ident.to_string();

    let discrim_type_name = attrs.discrim_type_name(&ty_name);
    let discrim_typespec = quote! { __xdrs::named_type(#discrim_type_name) };
    let discrim_var = attrs.discrim_var_name();

    let specs = item.variants.iter().filter_map(variant_case_spec);
    let case_specs = quote! { __xdrs::vec![#(#specs),*] };
    let default_variant = find_default_variant(item);
    let default_decl = option_tok(default_variant.map(|_v| {
        quote! {
            __xdrs::declare_void()
        }
    }));

    Ok(quote! {
        __xdrs::union_def(
            #ty_name,
            #discrim_var,
            #discrim_typespec,
            #case_specs,
            #default_decl,
            )
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
                let field_name = field
                    .ident
                    .as_ref()
                    .map(|i| i.to_string())
                    .unwrap_or("inner".to_string());
                quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#field_name)) }
            });
            if decls.len() == 1 {
                decls.next().unwrap()
            } else {
                // A variant with multiple named fields gets a struct
                // declaration containing them.
                quote! {
                    __xdrs::declare_type(#variant_name,
                        __xdrs::struct_type(__xdrs::vec![ #(#decls),* ]))
                }
            }
        }
        syn::Fields::Unnamed(_) => {
            unimplemented!();
        }
    }
}

// Generate a field declaration for an XDR struct
fn declare_struct_field(i: usize, field: &syn::Field) -> TokenStream {
    let ty = &field.ty;
    let ident = field.ident.clone().unwrap_or(format_ident!("f{}", i));
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
fn declare_enum_variant(index: usize, variant: &syn::Variant) -> TokenStream {
    if variant_is_default(variant) {
        // Ignore default arm
        return quote! {};
    }

    let ident = &variant.ident.to_string();

    let index = index as i32;
    let index_expr = quote! { #index };
    let discriminant_expr = variant
        .discriminant
        .as_ref()
        .map(|(_, expr)| expr.to_token_stream());
    let discriminant = discriminant_expr.as_ref().unwrap_or(&index_expr);

    quote! {
        (__xdrs::ident(#ident), __xdrs::signed(#discriminant)),
    }
}
