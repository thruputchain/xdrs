use crate::util::*;
use darling::*;
use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

#[derive(Clone, Debug, Default, FromDeriveInput, FromField)]
#[darling(attributes(xdrs), default)]
struct Attrs {
    pub into_type: Option<syn::Type>,
    pub via: Option<syn::Type>,

    #[darling(rename = "crate")]
    pub krate: Option<syn::Path>,
}

impl Attrs {
    fn crate_path(&self) -> syn::Path {
        self.krate
            .clone()
            .unwrap_or_else(|| syn::parse_quote!(::xdrs))
    }

    fn check_no_conversion<S: syn::spanned::Spanned>(&self, item: &S) -> Result<()> {
        if self.into_type.is_some() || self.via.is_some() {
            err(
                item,
                "ConstWriteXDR does not support `into_type` or `via` attributes, \
                 as the required conversions are not const-compatible.",
            )?;
        }
        Ok(())
    }
}

pub fn const_write_xdr(input: &syn::DeriveInput) -> Result<TokenStream> {
    let attrs = Attrs::from_derive_input(input)?;
    attrs.check_no_conversion(input)?;
    let krate = attrs.crate_path();

    let impls = const_write_data(input)?;

    Ok(quote! {
        const _: () = {
            use #krate::cxs;
            use #krate::txs;

            #impls
        };
    })
}

fn const_write_data(input: &syn::DeriveInput) -> Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return err(input, "ConstWriteXDR does not support generic types.");
    }

    match &input.data {
        syn::Data::Struct(item) => const_write_struct(input, item),
        syn::Data::Enum(item) => const_write_enum(input, item),
        _ => err(
            input,
            "ConstWriteXDR can only be derived for structs and enums",
        ),
    }
}

fn const_write_struct(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    let ty_ident = &input.ident;

    let cxs_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| const_write_field(i, f, Format::Cxs))
        .collect::<Result<Vec<_>>>()?;

    let txs_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| const_write_field(i, f, Format::Txs))
        .collect::<Result<Vec<_>>>()?;

    let cxs_size_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| const_size_field(i, f, Format::Cxs))
        .collect::<Result<Vec<_>>>()?;

    let txs_size_fields = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| const_size_field(i, f, Format::Txs))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl #ty_ident {
            pub const fn const_write_cxs<const __N: usize>(&self, w: &mut cxs::ConstWriter<__N>) {
                #(#cxs_fields)*
            }

            pub const fn const_write_txs<const __N: usize>(&self, w: &mut txs::ConstWriter<__N>) {
                #(#txs_fields)*
            }

            /// Returns the CXS encoded size of this value in bytes.
            pub const fn const_cxs_size(&self) -> usize {
                0 #(+ #cxs_size_fields)*
            }

            /// Returns the TXS encoded size of this value in bytes.
            pub const fn const_txs_size(&self) -> usize {
                0 #(+ #txs_size_fields)*
            }
        }
    })
}

#[derive(Copy, Clone)]
enum Format {
    Cxs,
    Txs,
}

fn const_size_field(index: usize, field: &syn::Field, fmt: Format) -> Result<TokenStream> {
    let attrs = Attrs::from_field(field)?;
    attrs.check_no_conversion(field)?;

    let fa = FieldAccess::new(index, field);
    let ty = &field.ty;

    Ok(const_size_field_expr(&fa, ty, fmt))
}

fn const_size_field_expr(fa: &FieldAccess<'_>, ty: &syn::Type, fmt: Format) -> TokenStream {
    let mod_path = match fmt {
        Format::Cxs => quote! { cxs::const_writer },
        Format::Txs => quote! { txs::const_writer },
    };
    let size_method = match fmt {
        Format::Cxs => quote! { const_cxs_size },
        Format::Txs => quote! { const_txs_size },
    };

    if let Some(size_fn) = primitive_size_fn(ty) {
        return quote! { #mod_path::#size_fn(self.#fa) };
    }

    if is_i128(ty) {
        return quote! {
            (#mod_path::size_i64((self.#fa >> 64) as i64)
             + #mod_path::size_u64((self.#fa & 0xffffffffffffffff) as u64))
        };
    }
    if is_u128(ty) {
        return quote! {
            (#mod_path::size_u64((self.#fa >> 64) as u64)
             + #mod_path::size_u64((self.#fa & 0xffffffffffffffff) as u64))
        };
    }

    if is_u8_array(ty) {
        return quote! { #mod_path::size_opaque_fixed(self.#fa.len()) };
    }

    if is_u8_slice_ref(ty) {
        return quote! { #mod_path::size_opaque_variable(self.#fa.len()) };
    }

    if is_str_ref(ty) {
        return quote! { #mod_path::size_opaque_variable(self.#fa.as_bytes().len()) };
    }

    // Nested type — call its const size method
    quote! { self.#fa.#size_method() }
}

fn const_write_field(index: usize, field: &syn::Field, fmt: Format) -> Result<TokenStream> {
    let attrs = Attrs::from_field(field)?;
    attrs.check_no_conversion(field)?;

    let fa = FieldAccess::new(index, field);
    let ty = &field.ty;

    Ok(const_write_field_expr(&fa, ty, fmt))
}

fn const_write_field_expr(fa: &FieldAccess<'_>, ty: &syn::Type, fmt: Format) -> TokenStream {
    let method = match fmt {
        Format::Cxs => quote! { const_write_cxs },
        Format::Txs => quote! { const_write_txs },
    };

    if let Some(writer_method) = primitive_write_method(ty) {
        return quote! { w.#writer_method(self.#fa); };
    }

    if is_i128(ty) {
        return quote! {
            w.write_i64((self.#fa >> 64) as i64);
            w.write_u64((self.#fa & 0xffffffffffffffff) as u64);
        };
    }
    if is_u128(ty) {
        return quote! {
            w.write_u64((self.#fa >> 64) as u64);
            w.write_u64((self.#fa & 0xffffffffffffffff) as u64);
        };
    }

    if is_u8_array(ty) {
        return quote! { w.write_opaque_fixed(&self.#fa); };
    }

    if is_u8_slice_ref(ty) {
        return quote! { w.write_opaque_variable(self.#fa); };
    }

    if is_str_ref(ty) {
        return quote! { w.write_opaque_variable(self.#fa.as_bytes()); };
    }

    // Default: assume the field type also derives ConstWriteXDR
    quote! { self.#fa.#method(w); }
}

fn const_write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item)?;

    let ty_ident = &input.ident;

    let cxs_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_write_variant(i, v, input, Format::Cxs))
        .collect::<Vec<_>>();

    let txs_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_write_variant(i, v, input, Format::Txs))
        .collect::<Vec<_>>();

    let cxs_size_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_size_variant(i, v, input, Format::Cxs))
        .collect::<Vec<_>>();

    let txs_size_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_size_variant(i, v, input, Format::Txs))
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #ty_ident {
            pub const fn const_write_cxs<const __N: usize>(&self, w: &mut cxs::ConstWriter<__N>) {
                match self {
                    #(#cxs_arms)*
                }
            }

            pub const fn const_write_txs<const __N: usize>(&self, w: &mut txs::ConstWriter<__N>) {
                match self {
                    #(#txs_arms)*
                }
            }

            /// Returns the CXS encoded size of this value in bytes.
            pub const fn const_cxs_size(&self) -> usize {
                match self {
                    #(#cxs_size_arms)*
                }
            }

            /// Returns the TXS encoded size of this value in bytes.
            pub const fn const_txs_size(&self) -> usize {
                match self {
                    #(#txs_size_arms)*
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::const_write_xdr;

    #[test]
    fn lifetime_only_generics_are_allowed() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Borrowed<'a> {
                items: &'a [Leaf],
                maybe: Option<Leaf>,
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(toks.contains("const_write_txs"));
        assert!(toks.contains("const_cxs_size"));
    }

    #[test]
    fn const_generics_are_allowed() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Fixed<const N: usize> {
                bytes: [u8; N],
                pair: (u32, u32),
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(toks.contains("const_write_cxs"));
        assert!(toks.contains("const_txs_size"));
    }

    #[test]
    fn type_generics_are_rejected() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Bad<T> {
                value: Option<T>,
            }
        };

        let err = const_write_xdr(&input).unwrap_err();
        assert!(err.to_string().contains("type parameters"));
    }
}

fn const_write_variant(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    fmt: Format,
) -> TokenStream {
    let is_default_arm = variant_is_default(v);
    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let var_bindings = const_var_bindings(&v.fields);

    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        quote! { #d }
    } else if is_default_arm {
        quote! { *_self_0 }
    } else {
        quote! { #variant_idx }
    };

    let write_discrim = quote! { w.write_i32(#discriminant); };

    let write_fields = if is_default_arm {
        quote! {}
    } else {
        let field_writes: Vec<_> = v
            .fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let bind = tmp_binding(i);
                let ty = &field.ty;
                const_write_binding_expr(&bind, ty, fmt)
            })
            .collect();
        quote! { #(#field_writes)* }
    };

    quote! {
        #ty_ident::#var_ident #var_bindings => {
            #write_discrim
            #write_fields
        }
    }
}

fn const_size_variant(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    fmt: Format,
) -> TokenStream {
    let is_default_arm = variant_is_default(v);
    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let var_bindings = const_var_bindings(&v.fields);

    let mod_path = match fmt {
        Format::Cxs => quote! { cxs::const_writer },
        Format::Txs => quote! { txs::const_writer },
    };

    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        quote! { #d }
    } else if is_default_arm {
        quote! { *_self_0 }
    } else {
        quote! { #variant_idx }
    };

    let discrim_size = quote! { #mod_path::size_i32(#discriminant) };

    let fields_size = if is_default_arm {
        quote! { 0 }
    } else {
        let field_sizes: Vec<_> = v
            .fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let bind = tmp_binding(i);
                let ty = &field.ty;
                const_size_binding_expr(&bind, ty, fmt)
            })
            .collect();
        if field_sizes.is_empty() {
            quote! { 0 }
        } else {
            quote! { 0 #(+ #field_sizes)* }
        }
    };

    quote! {
        #ty_ident::#var_ident #var_bindings => {
            #discrim_size + #fields_size
        }
    }
}

fn const_size_binding_expr(bind: &syn::Ident, ty: &syn::Type, fmt: Format) -> TokenStream {
    let mod_path = match fmt {
        Format::Cxs => quote! { cxs::const_writer },
        Format::Txs => quote! { txs::const_writer },
    };
    let size_method = match fmt {
        Format::Cxs => quote! { const_cxs_size },
        Format::Txs => quote! { const_txs_size },
    };

    if let Some(size_fn) = primitive_size_fn(ty) {
        return quote! { #mod_path::#size_fn(*#bind) };
    }

    if is_i128(ty) {
        return quote! {
            (#mod_path::size_i64((*#bind >> 64) as i64)
             + #mod_path::size_u64((*#bind & 0xffffffffffffffff) as u64))
        };
    }
    if is_u128(ty) {
        return quote! {
            (#mod_path::size_u64((*#bind >> 64) as u64)
             + #mod_path::size_u64((*#bind & 0xffffffffffffffff) as u64))
        };
    }

    if is_u8_array(ty) {
        return quote! { #mod_path::size_opaque_fixed(#bind.len()) };
    }

    if is_u8_slice_ref(ty) {
        return quote! { #mod_path::size_opaque_variable(#bind.len()) };
    }

    if is_str_ref(ty) {
        return quote! { #mod_path::size_opaque_variable(#bind.as_bytes().len()) };
    }

    quote! { #bind.#size_method() }
}

fn const_write_binding_expr(bind: &syn::Ident, ty: &syn::Type, fmt: Format) -> TokenStream {
    let method = match fmt {
        Format::Cxs => quote! { const_write_cxs },
        Format::Txs => quote! { const_write_txs },
    };

    if let Some(writer_method) = primitive_write_method(ty) {
        return quote! { w.#writer_method(*#bind); };
    }

    if is_i128(ty) {
        return quote! {
            w.write_i64((*#bind >> 64) as i64);
            w.write_u64((*#bind & 0xffffffffffffffff) as u64);
        };
    }
    if is_u128(ty) {
        return quote! {
            w.write_u64((*#bind >> 64) as u64);
            w.write_u64((*#bind & 0xffffffffffffffff) as u64);
        };
    }

    if is_u8_array(ty) {
        return quote! { w.write_opaque_fixed(#bind); };
    }

    if is_u8_slice_ref(ty) {
        return quote! { w.write_opaque_variable(#bind); };
    }

    if is_str_ref(ty) {
        return quote! { w.write_opaque_variable(#bind.as_bytes()); };
    }

    quote! { #bind.#method(w); }
}

fn const_var_bindings(fields: &syn::Fields) -> TokenStream {
    match fields {
        syn::Fields::Named(fields) => {
            let bindings = fields.named.iter().enumerate().map(|(i, field)| {
                let bind = tmp_binding(i);
                let ident = field.ident.as_ref().unwrap();
                quote! { #ident: #bind, }
            });
            quote! { { #(#bindings)* } }
        }
        syn::Fields::Unnamed(fields) => {
            let bindings = fields.unnamed.iter().enumerate().map(|(i, _)| {
                let bind = tmp_binding(i);
                quote! { #bind, }
            });
            quote! { ( #(#bindings)* ) }
        }
        syn::Fields::Unit => quote! {},
    }
}

// -- Type detection helpers --

fn primitive_size_fn(ty: &syn::Type) -> Option<TokenStream> {
    let path = match ty {
        syn::Type::Path(p) => p,
        _ => return None,
    };

    let ident = path.path.get_ident()?;
    let s = ident.to_string();

    match s.as_str() {
        "i32" => Some(quote! { size_i32 }),
        "u32" => Some(quote! { size_u32 }),
        "i64" => Some(quote! { size_i64 }),
        "u64" => Some(quote! { size_u64 }),
        "i16" => Some(quote! { size_i16 }),
        "u16" => Some(quote! { size_u16 }),
        "bool" => Some(quote! { size_bool }),
        _ => None,
    }
}

fn primitive_write_method(ty: &syn::Type) -> Option<TokenStream> {
    let path = match ty {
        syn::Type::Path(p) => p,
        _ => return None,
    };

    let ident = path.path.get_ident()?;
    let s = ident.to_string();

    match s.as_str() {
        "i32" => Some(quote! { write_i32 }),
        "u32" => Some(quote! { write_u32 }),
        "i64" => Some(quote! { write_i64 }),
        "u64" => Some(quote! { write_u64 }),
        "i16" => Some(quote! { write_i16 }),
        "u16" => Some(quote! { write_u16 }),
        "bool" => Some(quote! { write_bool }),
        _ => None,
    }
}

fn is_path_ident(ty: &syn::Type, name: &str) -> bool {
    if let syn::Type::Path(p) = ty
        && let Some(ident) = p.path.get_ident()
    {
        return ident == name;
    }
    false
}

fn is_i128(ty: &syn::Type) -> bool {
    is_path_ident(ty, "i128")
}

fn is_u128(ty: &syn::Type) -> bool {
    is_path_ident(ty, "u128")
}

fn is_u8_array(ty: &syn::Type) -> bool {
    if let syn::Type::Array(arr) = ty {
        return is_path_ident(&arr.elem, "u8");
    }
    false
}

fn is_u8_slice_ref(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(r) = ty
        && let syn::Type::Slice(s) = &*r.elem
    {
        return is_path_ident(&s.elem, "u8");
    }
    false
}

fn is_str_ref(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(r) = ty {
        return is_path_ident(&r.elem, "str");
    }
    false
}
