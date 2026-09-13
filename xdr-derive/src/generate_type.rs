use crate::util::*;
use darling::{FromDeriveInput, FromField, FromVariant};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::Result;

#[derive(Clone, Debug, FromField, FromDeriveInput, FromVariant)]
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

#[derive(Clone, Debug, Default, FromDeriveInput)]
#[darling(attributes(xdrs), default, allow_unknown_fields)]
struct CrateAttr {
    #[darling(rename = "crate")]
    krate: Option<syn::Path>,
}

impl Attrs {
    /// Returns the XDR name for the auto-generated discriminant enum.
    /// Defaults to `{RustEnumName}Type`.  Do not set `#[xdr(name)]` to
    /// this value — it will collide with the discriminant enum in the
    /// generated specification.
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
    let crate_attr = CrateAttr::from_derive_input(input)?;
    let krate = crate_attr
        .krate
        .unwrap_or_else(|| syn::parse_quote!(::xdrs));
    let imp = from_input(input)?;

    Ok(quote! {
        const _: () = {
            mod __xdrs {
                pub use #krate::*;
                pub use #krate::xdr_repr::*;
                extern crate alloc;
                pub use alloc::vec;
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

fn named_type_reference_methods(ty_name: String) -> TokenStream {
    quote! {
        fn type_ident() -> __xdrs::Ident {
            __xdrs::ident(#ty_name)
        }

        fn named_declaration(name: __xdrs::Ident) -> __xdrs::Declaration {
            __xdrs::Declaration::Named {
                name,
                ty: __xdrs::TypeSpec::Ident(__xdrs::ident(#ty_name)),
            }
        }
    }
}

fn from_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    // Structs with no fields are allowed per RFC4506:
    // ```
    // struct Empty {
    //    void;
    // };
    // ```
    // if item.fields.is_empty() {
    //     return Err(error(input, "XDR: can't represent a struct with no fields"));
    // }

    let attrs = Attrs::from_derive_input(input)?;
    let ty_ident = &input.ident;
    // The XDR struct has the same name as the rust type, unless the attrs override that
    let ty_name = attrs.name.clone().unwrap_or_else(|| ty_ident.to_string());
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let type_reference_methods = named_type_reference_methods(ty_name.clone());
    let declare_fields = if item.fields.is_empty() {
        vec![quote! { __xdrs::declare_void() }]
    } else {
        item.fields
            .iter()
            .enumerate()
            .map(|(i, field)| declare_struct_field(i, field))
            .collect::<Result<Vec<_>>>()?
    };

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

            #type_reference_methods
        }
    })
}

fn from_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item, has_xdr_discrim_type(input))?;

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
    let ty_name = attrs.name.clone().unwrap_or_else(|| ty_ident.to_string());
    let type_reference_methods = named_type_reference_methods(ty_name.clone());

    let enum_typedef = enum_typedef(input, item, attrs)?;

    // For a fieldless enum, we define an XDR enumeration
    Ok(quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                #enum_typedef
            }

            #type_reference_methods
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
    let ty_name = attrs.name.clone().unwrap_or_else(|| ty_ident.to_string());
    let type_reference_methods = named_type_reference_methods(ty_name.clone());

    let mut impls = vec![];

    // Generate a union typedef for this fieldful rust enum type
    let union_typedef = union_typedef(input, item, attrs)?;
    impls.push(quote! {
        impl #impl_generics __xdrs::HasTypeDef for #ty_ident #ty_generics #where_clause {
            fn type_def() -> __xdrs::TypeDef {
                #union_typedef
            }

            #type_reference_methods
        }
    });

    // We'll also implement HasSpecification for the type, as we may require
    // multiple definitions.  It will contain at least the union definition,
    // which, instead of generating again, we can get via the above HasTypeDef impl:
    let mut spec_definitions = vec![quote! { Self::def() }];

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

    let declare_variants: Vec<TokenStream> = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| declare_enum_variant(i, v))
        .collect::<Result<Vec<_>>>()?;

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
    // The XDR union has the same name as the rust type, unless the attrs override that
    let ty_name = attrs
        .name
        .clone()
        .unwrap_or_else(|| input.ident.to_string());

    // The discriminant type name is based on the Rust type name, not the custom XDR name
    let rust_enum_name = input.ident.to_string();
    let discrim_type_name = attrs.discrim_type_name(&rust_enum_name);
    let discrim_typespec = quote! { __xdrs::named_type(#discrim_type_name) };
    let discrim_var = attrs.discrim_var_name();

    let specs: Vec<TokenStream> = item
        .variants
        .iter()
        .map(variant_case_spec)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    let case_specs = quote! { __xdrs::vec![#(#specs),*] };
    let default_variant = find_default_variant(item);
    let default_decl = match default_variant {
        Some(v) => option_tok(Some(default_variant_payload_decl(v)?)),
        None => option_tok(None),
    };

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

fn variant_case_spec(variant: &syn::Variant) -> Result<Option<TokenStream>> {
    if variant_is_default(variant) {
        return Ok(None);
    }

    let attrs = Attrs::from_variant(variant)?;
    let variant_name = attrs.name.unwrap_or_else(|| variant.ident.to_string());
    let decl = variant_fields_decl(variant)?;

    Ok(Some(quote! {
        __xdrs::single_case_spec(#variant_name, #decl)
    }))
}

fn variant_fields_decl(variant: &syn::Variant) -> Result<TokenStream> {
    let fields = &variant.fields;
    let variant_name = variant.ident.to_string().to_lowercase();

    match fields {
        syn::Fields::Unit => Ok(quote! { __xdrs::Declaration::Void }),
        syn::Fields::Named(fields_named) => {
            if fields_named.named.is_empty() {
                return Ok(quote! { __xdrs::Declaration::Void });
            }

            let mut decls = fields_named.named.iter().map(|field| {
                let field_ty = &field.ty;
                let attrs = Attrs::from_field(field)?;
                let field_name = attrs.name.unwrap_or_else(|| {
                    field.ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or("inner".to_string())
                });
                Ok(quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#field_name)) })
            }).collect::<Result<Vec<_>>>()?;
            if decls.len() == 1 {
                Ok(decls.pop().unwrap())
            } else {
                Ok(quote! {
                    __xdrs::declare_type(#variant_name,
                        __xdrs::struct_type(__xdrs::vec![ #(#decls),* ]))
                })
            }
        }
        syn::Fields::Unnamed(fields_unnamed) => {
            if fields_unnamed.unnamed.is_empty() {
                return Ok(quote! { __xdrs::Declaration::Void });
            }

            let decls = fields_unnamed.unnamed.iter().enumerate().map(|(i, field)| {
                let field_ty = &field.ty;
                let field_name = format!("f{i}");
                quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#field_name)) }
            });
            if decls.len() == 1 {
                let field_ty = &fields_unnamed.unnamed[0].ty;
                Ok(quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#variant_name)) })
            } else {
                Ok(quote! {
                    __xdrs::declare_type(#variant_name,
                        __xdrs::struct_type(__xdrs::vec![ #(#decls),* ]))
                })
            }
        }
    }
}

fn default_variant_payload_decl(variant: &syn::Variant) -> Result<TokenStream> {
    match &variant.fields {
        syn::Fields::Named(fields_named) => {
            let payload_fields: Vec<_> = fields_named.named.iter().skip(1).collect();
            if payload_fields.is_empty() {
                return Ok(quote! { __xdrs::declare_void() });
            }
            let variant_name = variant.ident.to_string().to_lowercase();
            let decls = payload_fields
                .iter()
                .map(|field| {
                    let field_ty = &field.ty;
                    let attrs = Attrs::from_field(field)?;
                    let field_name = attrs.name.unwrap_or_else(|| {
                        field.ident
                            .as_ref()
                            .map(|i| i.to_string())
                            .unwrap_or("inner".to_string())
                    });
                    Ok(quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#field_name)) })
                })
                .collect::<Result<Vec<_>>>()?;
            if decls.len() == 1 {
                Ok(decls.into_iter().next().unwrap())
            } else {
                Ok(quote! {
                    __xdrs::declare_type(#variant_name,
                        __xdrs::struct_type(__xdrs::vec![ #(#decls),* ]))
                })
            }
        }
        syn::Fields::Unnamed(fields_unnamed) => {
            let payload_fields: Vec<_> = fields_unnamed.unnamed.iter().skip(1).collect();
            if payload_fields.is_empty() {
                return Ok(quote! { __xdrs::declare_void() });
            }
            let variant_name = variant.ident.to_string().to_lowercase();
            let decls: Vec<_> = payload_fields
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let field_ty = &field.ty;
                    let field_name = format!("f{i}");
                    quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#field_name)) }
                })
                .collect();
            if decls.len() == 1 {
                let field_ty = &payload_fields[0].ty;
                Ok(quote! { <#field_ty as __xdrs::Declare>::declare(__xdrs::ident(#variant_name)) })
            } else {
                Ok(quote! {
                    __xdrs::declare_type(#variant_name,
                        __xdrs::struct_type(__xdrs::vec![ #(#decls),* ]))
                })
            }
        }
        syn::Fields::Unit => Ok(quote! { __xdrs::declare_void() }),
    }
}

// Generate a field declaration for an XDR struct
fn declare_struct_field(i: usize, field: &syn::Field) -> Result<TokenStream> {
    let ty = &field.ty;
    let attrs = Attrs::from_field(field)?;
    let ident = field.ident.clone().unwrap_or(format_ident!("f{}", i));
    let name = attrs.name.unwrap_or_else(|| ident.to_string());

    Ok(quote! {
        {
            <#ty as __xdrs::Declare>::declare(
                __xdrs::ident(#name))
        },
    })
}

// Generate a variant declaration for an XDR enumeration
fn declare_enum_variant(index: usize, variant: &syn::Variant) -> Result<TokenStream> {
    if variant_is_default(variant) {
        return Ok(quote! {});
    }

    let attrs = Attrs::from_variant(variant)?;
    let ident = attrs.name.unwrap_or_else(|| variant.ident.to_string());

    let index = index as i32;
    let index_expr = quote! { #index };
    let discriminant_expr = variant
        .discriminant
        .as_ref()
        .map(|(_, expr)| expr.to_token_stream());
    let discriminant = discriminant_expr.as_ref().unwrap_or(&index_expr);

    Ok(quote! {
        (__xdrs::ident(#ident), __xdrs::signed(#discriminant)),
    })
}
