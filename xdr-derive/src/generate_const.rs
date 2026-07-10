use crate::util::*;
use darling::*;
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
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
    if input
        .generics
        .params
        .iter()
        .any(|param| matches!(param, syn::GenericParam::Type(_)))
    {
        return err(input, "ConstWriteXDR does not support type parameters.");
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
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

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

    let cxs_max_size = const_max_fields_expr(&item.fields, Format::Cxs)?;
    let txs_max_size = const_max_fields_expr(&item.fields, Format::Txs)?;

    let buf = fresh_buf_size_ident(&input.generics);

    Ok(quote! {
        impl #impl_generics #ty_ident #ty_generics #where_clause {
            pub const fn const_write_cxs<const #buf: usize>(&self, w: &mut cxs::ConstWriter<#buf>) {
                #(#cxs_fields)*
            }

            pub const fn const_write_txs<const #buf: usize>(&self, w: &mut txs::ConstWriter<#buf>) {
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

            /// Returns the worst-case CXS encoded size over all values of this
            /// type, or `None` if the type is unbounded, possibly recursive
            /// (reached through indirection), or its bound overflows `usize`.
            pub const fn const_max_cxs_size() -> ::core::option::Option<usize> {
                #cxs_max_size
            }

            /// Returns the worst-case TXS encoded size over all values of this
            /// type, or `None` if the type is unbounded, possibly recursive
            /// (reached through indirection), or its bound overflows `usize`.
            pub const fn const_max_txs_size() -> ::core::option::Option<usize> {
                #txs_max_size
            }
        }
    })
}

#[derive(Copy, Clone)]
enum Format {
    Cxs,
    Txs,
}

impl Format {
    fn const_writer_path(self) -> TokenStream {
        match self {
            Format::Cxs => quote! { cxs::const_writer },
            Format::Txs => quote! { txs::const_writer },
        }
    }
}

fn fresh_buf_size_ident(generics: &syn::Generics) -> syn::Ident {
    let used: HashSet<String> = generics
        .params
        .iter()
        .filter_map(|p| match p {
            syn::GenericParam::Const(c) => Some(c.ident.to_string()),
            _ => None,
        })
        .collect();

    let base = "__N";
    if !used.contains(base) {
        return syn::Ident::new(base, proc_macro2::Span::call_site());
    }
    for i in 0u64.. {
        let candidate = format!("{base}{i}");
        if !used.contains(&candidate) {
            return syn::Ident::new(&candidate, proc_macro2::Span::call_site());
        }
    }
    unreachable!()
}

fn const_size_field(index: usize, field: &syn::Field, fmt: Format) -> Result<TokenStream> {
    let attrs = Attrs::from_field(field)?;
    attrs.check_no_conversion(field)?;

    let fa = FieldAccess::new(index, field);
    const_size_value_expr(quote! { &self.#fa }, &field.ty, fmt)
}

fn const_size_value_expr(
    expr_ref: TokenStream,
    ty: &syn::Type,
    fmt: Format,
) -> Result<TokenStream> {
    check_supported_const_shape(ty)?;
    let ty = transparent_type(ty);

    let mod_path = match fmt {
        Format::Cxs => quote! { cxs::const_writer },
        Format::Txs => quote! { txs::const_writer },
    };
    let size_method = match fmt {
        Format::Cxs => quote! { const_cxs_size },
        Format::Txs => quote! { const_txs_size },
    };

    if let Some(size_fn) = primitive_size_fn(ty) {
        return Ok(quote! { #mod_path::#size_fn(*#expr_ref) });
    }

    if is_u8_array(ty) {
        return Ok(quote! { #mod_path::size_opaque_fixed((#expr_ref).len()) });
    }

    if is_u8_slice_ref(ty) {
        return Ok(quote! { #mod_path::size_opaque_variable((*#expr_ref).len()) });
    }

    if is_str_ref(ty) {
        return Ok(quote! { #mod_path::size_opaque_variable((*#expr_ref).as_bytes().len()) });
    }

    if let Some(max) = bytesm_stringm_max(ty) {
        let max_guard = max_len_guard(&max);
        return Ok(quote! {
            {
                let slice = (#expr_ref).as_slice();
                #max_guard
                #mod_path::size_opaque_variable(slice.len())
            }
        });
    }

    if let Some((elem, max)) = vecm_parts(ty) {
        let max_guard = max_len_guard(&max);
        if is_path_ident(elem, "u8") {
            return Ok(quote! {
                {
                    let slice = (#expr_ref).as_slice();
                    #max_guard
                    #mod_path::size_opaque_variable(slice.len())
                }
            });
        }
        let inner_size = const_size_value_expr(quote! { &slice[i] }, elem, fmt)?;
        return Ok(quote! {
            {
                let slice = (#expr_ref).as_slice();
                ::core::assert!(slice.len() <= ::core::primitive::u32::MAX as usize, "slice length exceeds u32::MAX");
                #max_guard
                let mut total = #mod_path::size_u32(slice.len() as u32);
                let mut i = 0;
                while i < slice.len() {
                    total += #inner_size;
                    i += 1;
                }
                total
            }
        });
    }

    if is_phantom_data(ty) {
        return Ok(quote! { 0 });
    }

    if is_infallible(ty) {
        return Ok(quote! { 0 });
    }

    if let Some(inner) = option_inner(ty) {
        let inner_size = const_size_value_expr(quote! { _inner }, inner, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Some(_inner) => #mod_path::size_i32(1) + #inner_size,
                None => #mod_path::size_i32(0),
            }
        });
    }

    if let Some((ok_ty, err_ty)) = result_inners(ty) {
        let ok_size = const_size_value_expr(quote! { _inner }, ok_ty, fmt)?;
        let err_size = const_size_value_expr(quote! { _inner }, err_ty, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Ok(_inner) => #mod_path::size_i32(0) + #ok_size,
                Err(_inner) => #mod_path::size_i32(1) + #err_size,
            }
        });
    }

    if let Some(inner) = slice_ref_inner(ty) {
        let inner_size = const_size_value_expr(quote! { &slice[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let slice = *#expr_ref;
                ::core::assert!(slice.len() <= ::core::primitive::u32::MAX as usize, "slice length exceeds u32::MAX");
                let mut total = #mod_path::size_u32(slice.len() as u32);
                let mut i = 0;
                while i < slice.len() {
                    total += #inner_size;
                    i += 1;
                }
                total
            }
        });
    }

    if let Some((inner, _)) = array_inner(ty) {
        let inner_size = const_size_value_expr(quote! { &arr[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let arr = #expr_ref;
                ::core::assert!(arr.len() <= ::core::primitive::u32::MAX as usize, "array length exceeds u32::MAX");
                let mut total = 0;
                let mut i = 0;
                while i < arr.len() {
                    total += #inner_size;
                    i += 1;
                }
                total
            }
        });
    }

    if let Some(elems) = tuple_elems(ty) {
        let elem_sizes = elems
            .iter()
            .enumerate()
            .map(|(i, elem)| const_size_value_expr(tuple_elem_ref(&expr_ref, i), elem, fmt))
            .collect::<Result<Vec<_>>>()?;
        return Ok(quote! { 0 #(+ #elem_sizes)* });
    }

    if let syn::Type::Reference(r) = ty {
        return const_size_value_expr(quote! { *#expr_ref }, &r.elem, fmt);
    }

    Ok(quote! { (#expr_ref).#size_method() })
}

/// Folds the worst-case size bounds of `fields` into a single
/// `Option<usize>` expression via `max_size_add`, starting from `Some(0)`.
fn const_max_fields_expr(fields: &syn::Fields, fmt: Format) -> Result<TokenStream> {
    let mod_path = fmt.const_writer_path();
    let mut acc = quote! { ::core::option::Option::Some(0usize) };
    for field in fields {
        let bound = const_max_type_expr(&field.ty, fmt)?;
        acc = quote! { #mod_path::max_size_add(#acc, #bound) };
    }
    Ok(acc)
}

/// Produces an `Option<usize>` expression for the worst-case encoded size of
/// `ty` over all its values.
///
/// Bounds fold through by-value layout only: any shape reached through a
/// pointer (`&T`, `&[T]`, `&str`, and the heap-backed element-generic
/// `VecM<T, MAX>`) yields literal `None` without naming the pointee's
/// `const_max_*_size` fns, so const-eval cycles for recursive types are
/// impossible by construction. Byte-shaped bounded containers (`BytesM`,
/// `StringM`, `VecM<u8, MAX>`) keep exact bounds; a `MAX` that is not spelled
/// out defaults to `u32::MAX`, matching the `xdr_common` type defaults.
fn const_max_type_expr(ty: &syn::Type, fmt: Format) -> Result<TokenStream> {
    check_supported_const_shape(ty)?;
    let ty = transparent_type(ty);

    let mod_path = fmt.const_writer_path();
    let max_method = match fmt {
        Format::Cxs => quote! { const_max_cxs_size },
        Format::Txs => quote! { const_max_txs_size },
    };
    let none = quote! { ::core::option::Option::None };

    if let Some(max_size_fn) = primitive_max_size_fn(ty) {
        return Ok(quote! { ::core::option::Option::Some(#mod_path::#max_size_fn()) });
    }

    if let Some((_, len)) = array_inner(ty).filter(|(elem, _)| is_path_ident(elem, "u8")) {
        return Ok(quote! { #mod_path::max_size_opaque_fixed(#len) });
    }

    // Unbounded lengths: no static cap exists for slices and strings.
    if is_u8_slice_ref(ty) || is_str_ref(ty) {
        return Ok(none);
    }

    if let Some(max) = bytesm_stringm_max(ty) {
        let max = max_or_default(max);
        return Ok(quote! { #mod_path::max_size_opaque_variable(#max) });
    }

    if let Some((elem, max)) = vecm_parts(ty) {
        if is_path_ident(elem, "u8") {
            let max = max_or_default(max);
            return Ok(quote! { #mod_path::max_size_opaque_variable(#max) });
        }
        // Pointer-cut: a heap-backed container generic over its element type
        // is a cycle edge for recursive types, so the element-generic fold is
        // literal `None` (never naming the element's bound).
        return Ok(none);
    }

    if is_phantom_data(ty) || is_infallible(ty) {
        return Ok(quote! { ::core::option::Option::Some(0usize) });
    }

    if let Some(inner) = option_inner(ty) {
        let inner_bound = const_max_type_expr(inner, fmt)?;
        return Ok(quote! {
            #mod_path::max_size_add(
                ::core::option::Option::Some(#mod_path::size_i32(1)),
                #inner_bound,
            )
        });
    }

    if let Some((ok_ty, err_ty)) = result_inners(ty) {
        let ok_bound = const_max_type_expr(ok_ty, fmt)?;
        let err_bound = const_max_type_expr(err_ty, fmt)?;
        return Ok(quote! {
            #mod_path::max_size_add(
                ::core::option::Option::Some(#mod_path::size_i32(1)),
                #mod_path::max_size_max(#ok_bound, #err_bound),
            )
        });
    }

    // Pointer-cut for slices (also unbounded in length).
    if slice_ref_inner(ty).is_some() {
        return Ok(none);
    }

    if let Some((elem, len)) = array_inner(ty) {
        let elem_bound = const_max_type_expr(elem, fmt)?;
        return Ok(quote! { #mod_path::max_size_array(#len, #elem_bound) });
    }

    if let Some(elems) = tuple_elems(ty) {
        let mut acc = quote! { ::core::option::Option::Some(0usize) };
        for elem in elems {
            let bound = const_max_type_expr(elem, fmt)?;
            acc = quote! { #mod_path::max_size_add(#acc, #bound) };
        }
        return Ok(acc);
    }

    // Pointer-cut: boundedness does not cross a reference.
    if matches!(ty, syn::Type::Reference(_)) {
        return Ok(none);
    }

    // Named by-value leaf: inline recursion is unrepresentable in Rust, so
    // naming the leaf's bound cannot form a const-eval cycle.
    Ok(quote! { <#ty>::#max_method() })
}

/// Splices an explicit `MAX` bound, or the `u32::MAX` default used by
/// `xdr_common`'s `BytesM`/`StringM`/`VecM` when none is spelled out.
fn max_or_default(max: Option<TokenStream>) -> TokenStream {
    max.unwrap_or_else(|| quote! { ::core::primitive::u32::MAX })
}

fn const_write_field(index: usize, field: &syn::Field, fmt: Format) -> Result<TokenStream> {
    let attrs = Attrs::from_field(field)?;
    attrs.check_no_conversion(field)?;

    let fa = FieldAccess::new(index, field);
    const_write_value_expr(quote! { &self.#fa }, &field.ty, fmt)
}

fn const_write_value_expr(
    expr_ref: TokenStream,
    ty: &syn::Type,
    fmt: Format,
) -> Result<TokenStream> {
    check_supported_const_shape(ty)?;
    let ty = transparent_type(ty);

    let method = match fmt {
        Format::Cxs => quote! { const_write_cxs },
        Format::Txs => quote! { const_write_txs },
    };

    if let Some(writer_method) = primitive_write_method(ty) {
        return Ok(quote! { w.#writer_method(*#expr_ref); });
    }

    if is_u8_array(ty) {
        return Ok(quote! { w.write_opaque_fixed(#expr_ref); });
    }

    if is_u8_slice_ref(ty) {
        return Ok(quote! { w.write_opaque_variable(*#expr_ref); });
    }

    if is_str_ref(ty) {
        return Ok(quote! { w.write_opaque_variable((*#expr_ref).as_bytes()); });
    }

    if let Some(max) = bytesm_stringm_max(ty) {
        let max_guard = max_len_guard(&max);
        return Ok(quote! {
            {
                let slice = (#expr_ref).as_slice();
                #max_guard
                w.write_opaque_variable(slice);
            }
        });
    }

    if let Some((elem, max)) = vecm_parts(ty) {
        let max_guard = max_len_guard(&max);
        if is_path_ident(elem, "u8") {
            return Ok(quote! {
                {
                    let slice = (#expr_ref).as_slice();
                    #max_guard
                    w.write_opaque_variable(slice);
                }
            });
        }
        let inner_write = const_write_value_expr(quote! { &slice[i] }, elem, fmt)?;
        return Ok(quote! {
            {
                let slice = (#expr_ref).as_slice();
                ::core::assert!(slice.len() <= ::core::primitive::u32::MAX as usize, "slice length exceeds u32::MAX");
                #max_guard
                w.write_u32(slice.len() as u32);
                let mut i = 0;
                while i < slice.len() {
                    #inner_write
                    i += 1;
                }
            }
        });
    }

    if is_phantom_data(ty) {
        return Ok(quote! {});
    }

    if is_infallible(ty) {
        return Ok(quote! {});
    }

    if let Some(inner) = option_inner(ty) {
        let inner_write = const_write_value_expr(quote! { _inner }, inner, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Some(_inner) => {
                    w.write_i32(1);
                    #inner_write
                }
                None => w.write_i32(0),
            }
        });
    }

    if let Some((ok_ty, err_ty)) = result_inners(ty) {
        let ok_write = const_write_value_expr(quote! { _inner }, ok_ty, fmt)?;
        let err_write = const_write_value_expr(quote! { _inner }, err_ty, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Ok(_inner) => {
                    w.write_i32(0);
                    #ok_write
                }
                Err(_inner) => {
                    w.write_i32(1);
                    #err_write
                }
            }
        });
    }

    if let Some(inner) = slice_ref_inner(ty) {
        let inner_write = const_write_value_expr(quote! { &slice[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let slice = *#expr_ref;
                ::core::assert!(slice.len() <= ::core::primitive::u32::MAX as usize, "slice length exceeds u32::MAX");
                w.write_u32(slice.len() as u32);
                let mut i = 0;
                while i < slice.len() {
                    #inner_write
                    i += 1;
                }
            }
        });
    }

    if let Some((inner, _)) = array_inner(ty) {
        let inner_write = const_write_value_expr(quote! { &arr[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let arr = #expr_ref;
                ::core::assert!(arr.len() <= ::core::primitive::u32::MAX as usize, "array length exceeds u32::MAX");
                let mut i = 0;
                while i < arr.len() {
                    #inner_write
                    i += 1;
                }
            }
        });
    }

    if let Some(elems) = tuple_elems(ty) {
        let elem_writes = elems
            .iter()
            .enumerate()
            .map(|(i, elem)| const_write_value_expr(tuple_elem_ref(&expr_ref, i), elem, fmt))
            .collect::<Result<Vec<_>>>()?;
        return Ok(quote! { #(#elem_writes)* });
    }

    if let syn::Type::Reference(r) = ty {
        return const_write_value_expr(quote! { *#expr_ref }, &r.elem, fmt);
    }

    Ok(quote! { (#expr_ref).#method(w); })
}

fn const_write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item, has_xdr_discrim_type(input))?;

    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let cxs_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_write_variant(i, v, input, Format::Cxs))
        .collect::<Result<Vec<_>>>()?;

    let txs_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_write_variant(i, v, input, Format::Txs))
        .collect::<Result<Vec<_>>>()?;

    let cxs_size_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_size_variant(i, v, input, Format::Cxs))
        .collect::<Result<Vec<_>>>()?;

    let txs_size_arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_size_variant(i, v, input, Format::Txs))
        .collect::<Result<Vec<_>>>()?;

    let cxs_max_size = const_max_variants_expr(item, Format::Cxs)?;
    let txs_max_size = const_max_variants_expr(item, Format::Txs)?;

    let buf = fresh_buf_size_ident(&input.generics);

    Ok(quote! {
        impl #impl_generics #ty_ident #ty_generics #where_clause {
            pub const fn const_write_cxs<const #buf: usize>(&self, w: &mut cxs::ConstWriter<#buf>) {
                match self {
                    #(#cxs_arms)*
                }
            }

            pub const fn const_write_txs<const #buf: usize>(&self, w: &mut txs::ConstWriter<#buf>) {
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

            /// Returns the worst-case CXS encoded size over all values of this
            /// type, or `None` if the type is unbounded, possibly recursive
            /// (reached through indirection), or its bound overflows `usize`.
            pub const fn const_max_cxs_size() -> ::core::option::Option<usize> {
                #cxs_max_size
            }

            /// Returns the worst-case TXS encoded size over all values of this
            /// type, or `None` if the type is unbounded, possibly recursive
            /// (reached through indirection), or its bound overflows `usize`.
            pub const fn const_max_txs_size() -> ::core::option::Option<usize> {
                #txs_max_size
            }
        }
    })
}

/// Folds the worst-case size bound of an enum: the maximum over its variants
/// of (discriminant size + field bounds). Uninhabited enums bound to
/// `Some(0)`; a single unbounded variant makes the whole bound `None`.
fn const_max_variants_expr(item: &syn::DataEnum, fmt: Format) -> Result<TokenStream> {
    let mod_path = fmt.const_writer_path();
    let variant_bounds = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| const_max_variant_expr(i, v, fmt))
        .collect::<Result<Vec<_>>>()?;

    let mut bounds = variant_bounds.into_iter();
    let Some(mut acc) = bounds.next() else {
        return Ok(quote! { ::core::option::Option::Some(0usize) });
    };
    for bound in bounds {
        acc = quote! { #mod_path::max_size_max(#acc, #bound) };
    }
    Ok(acc)
}

fn const_max_variant_expr(index: usize, v: &syn::Variant, fmt: Format) -> Result<TokenStream> {
    let is_default_arm = variant_is_default(v);
    let mod_path = fmt.const_writer_path();

    let variant_idx = index as i32;
    let discrim_bound = if let Some((_, d)) = v.discriminant.as_ref() {
        quote! { #mod_path::size_i32(#d) }
    } else if is_default_arm {
        // The default arm's discriminant is a runtime value.
        quote! { #mod_path::max_size_i32() }
    } else {
        quote! { #mod_path::size_i32(#variant_idx) }
    };

    let mut acc = quote! { ::core::option::Option::Some(#discrim_bound) };
    for (i, field) in v.fields.iter().enumerate() {
        // The default arm's first field is the discriminant itself.
        if is_default_arm && i == 0 {
            continue;
        }
        let bound = const_max_type_expr(&field.ty, fmt)?;
        acc = quote! { #mod_path::max_size_add(#acc, #bound) };
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::const_write_xdr;

    fn normalize_tokens(tokens: &str) -> String {
        tokens.chars().filter(|c| !c.is_whitespace()).collect()
    }

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
    fn recursive_concrete_shapes_are_allowed() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct RecursiveShapes {
                maybe: Option<Leaf>,
                nested: Option<Option<Leaf>>,
                choice: Result<Leaf, Leaf>,
                items: [Leaf; 2],
                pair: (Leaf, Leaf),
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(toks.contains("match&self.maybe{Some(_inner)=>"));
        assert!(toks.contains("match&self.nested{Some(_inner)=>"));
        assert!(toks.contains("match&self.choice{Ok(_inner)=>"));
        assert!(toks.contains("whilei<arr.len()"));
        assert!(toks.contains("&(*&self.pair).0"));
        assert!(toks.contains("&(*&self.pair).1"));
    }

    #[test]
    fn borrowed_tuple_recursion_dereferences_before_indexing() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct BorrowedTupleRecursion {
                maybe_pair: Option<(Leaf, (Leaf, Leaf))>,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(toks.contains("&(*_inner).0"));
        assert!(toks.contains("&(*&(*_inner).1).0"));
        assert!(!toks.contains("&&_inner.0"));
    }

    #[test]
    fn const_generic_named_dunder_n_does_not_collide() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Packet<const __N: usize> {
                bytes: [u8; __N],
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(toks.contains("const_write_cxs"));
        assert!(toks.contains("const_write_txs"));
        assert!(
            toks.contains("const __N0 : usize"),
            "buffer generic should be renamed to avoid collision with user's __N"
        );
    }

    #[test]
    fn const_generic_named_dunder_n_and_n0_skips_to_n1() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Packet<const __N: usize, const __N0: usize> {
                a: [u8; __N],
                b: [u8; __N0],
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(
            toks.contains("const __N1 : usize"),
            "buffer generic should skip to __N1 when __N and __N0 are both taken"
        );
    }

    #[test]
    fn vecm_bytesm_stringm_are_recognized() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Containers {
                nums: VecM<u32, 5>,
                raw: VecM<u8, 4>,
                bytes: BytesM<4>,
                name: StringM<8>,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        // VecM<u32, 5> writes a length prefix and iterates elements.
        assert!(toks.contains("w.write_u32(slice.len()asu32)"));
        assert!(toks.contains("whilei<slice.len()"));
        // VecM<u8, _>, BytesM, and StringM take the opaque fast path.
        assert!(toks.contains("w.write_opaque_variable(slice)"));
        // Explicit MAX bounds become length guards.
        assert!(toks.contains("slice.len()<=(5)asusize"));
        assert!(toks.contains("slice.len()<=(4)asusize"));
        assert!(toks.contains("slice.len()<=(8)asusize"));
    }

    #[test]
    fn vecm_without_explicit_max_has_no_max_guard() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Unbounded {
                items: VecM<u32>,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(!toks.contains("lengthexceedsMAX"));
    }

    #[test]
    fn vecm_max_accepts_const_generic_param() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Bounded<const M: u32> {
                items: VecM<u32, M>,
                blob: BytesM<M>,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(toks.contains("slice.len()<=(M)asusize"));
    }

    #[test]
    fn nested_vecm_recurses() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Nested {
                items: VecM<VecM<u32, 2>, 3>,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        // The inner VecM is reached through the outer element loop.
        assert!(toks.contains("(&slice[i]).as_slice()"));
    }

    #[test]
    fn max_size_fns_cut_at_pointers() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Cut<'a> {
                by_ref: &'a Leaf,
                items: &'a [Leaf],
                raw: &'a [u8],
                name: &'a str,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(toks.contains("pubconstfnconst_max_cxs_size()"));
        assert!(toks.contains("pubconstfnconst_max_txs_size()"));
        // Every field is a pointer cut: only `None` bounds are folded, and the
        // max fns never name the pointee's bound.
        assert!(toks.contains(
            "max_size_add(::core::option::Option::Some(0usize),::core::option::Option::None)"
        ));
        assert!(!toks.contains("Leaf>::const_max_cxs_size"));
    }

    #[test]
    fn max_size_fns_fold_by_value_shapes() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Folded {
                n: u64,
                fixed: [u8; 8],
                arr: [Leaf; 3],
                maybe: Option<u32>,
                choice: Result<u32, Leaf>,
                blob: BytesM<16>,
                raw: VecM<u8, 4>,
                unbounded_blob: BytesM,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(toks.contains("max_size_u64()"));
        assert!(toks.contains("max_size_opaque_fixed(8)"));
        assert!(toks.contains("max_size_array(3,<Leaf>::const_max_cxs_size())"));
        assert!(toks.contains("max_size_array(3,<Leaf>::const_max_txs_size())"));
        assert!(toks.contains("max_size_max("));
        assert!(toks.contains("max_size_opaque_variable(16)"));
        assert!(toks.contains("max_size_opaque_variable(4)"));
        // BytesM without an explicit MAX defaults to u32::MAX.
        assert!(toks.contains("max_size_opaque_variable(::core::primitive::u32::MAX)"));
    }

    #[test]
    fn max_size_fns_cut_at_element_generic_vecm() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Items {
                items: VecM<Leaf, 5>,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        // Element-generic VecM is a pointer cut, even with an explicit MAX:
        // the max fns never name the element's bound.
        assert!(!toks.contains("::const_max_cxs_size()"));
        assert!(!toks.contains("::const_max_txs_size()"));
        assert!(toks.contains(
            "max_size_add(::core::option::Option::Some(0usize),::core::option::Option::None)"
        ));
    }

    #[test]
    fn max_size_fns_take_max_over_enum_variants() {
        let input: syn::DeriveInput = syn::parse_quote! {
            #[repr(i32)]
            enum Choice {
                A(u64) = 7,
                B(u32, u32) = 9,
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(toks.contains("max_size_max("));
        assert!(toks.contains("size_i32(7)"));
        assert!(toks.contains("size_i32(9)"));
    }

    #[test]
    fn max_size_fns_default_variant_uses_worst_case_discriminant() {
        let input: syn::DeriveInput = syn::parse_quote! {
            #[repr(i32)]
            enum WithDefault {
                Known(u32),
                #[xdr(default_variant)]
                Other(i32),
            }
        };

        let toks = normalize_tokens(&const_write_xdr(&input).unwrap().to_string());
        assert!(toks.contains("max_size_i32()"));
    }

    #[test]
    fn type_generics_are_rejected() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Bad<T> {
                value: Option<T>,
            }
        };

        let err = const_write_xdr(&input).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("generic types") || msg.contains("type parameters"));
    }

    #[test]
    fn array_guard_uses_qualified_u32_max() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct WithArray {
                items: [Leaf; 3],
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(
            toks.contains(":: core :: primitive :: u32 :: MAX"),
            "array guard must use fully qualified ::core::primitive::u32::MAX, got:\n{toks}"
        );
        let bare = toks.replace(":: core :: primitive :: u32 :: MAX", "");
        assert!(
            !bare.contains("u32 :: MAX"),
            "no bare u32::MAX should remain in generated code"
        );
    }

    #[test]
    fn slice_guard_uses_qualified_u32_max() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct WithSlice<'a> {
                items: &'a [Leaf],
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(
            toks.contains(":: core :: primitive :: u32 :: MAX"),
            "slice guard must use fully qualified ::core::primitive::u32::MAX, got:\n{toks}"
        );
        let bare = toks.replace(":: core :: primitive :: u32 :: MAX", "");
        assert!(
            !bare.contains("u32 :: MAX"),
            "no bare u32::MAX should remain in generated code"
        );
    }
}

fn const_write_variant(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    fmt: Format,
) -> Result<TokenStream> {
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

    let field_writes: Vec<_> = v
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            if is_default_arm && i == 0 {
                Ok(quote! {})
            } else {
                let attrs = Attrs::from_field(field)?;
                attrs.check_no_conversion(field)?;
                let bind = tmp_binding(i);
                let ty = &field.ty;
                const_write_binding_expr(&bind, ty, fmt)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let write_fields = quote! { #(#field_writes)* };

    Ok(quote! {
        #ty_ident::#var_ident #var_bindings => {
            #write_discrim
            #write_fields
        }
    })
}

fn const_size_variant(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    fmt: Format,
) -> Result<TokenStream> {
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

    let field_sizes: Vec<_> = v
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            if is_default_arm && i == 0 {
                Ok(quote! { 0 })
            } else {
                let attrs = Attrs::from_field(field)?;
                attrs.check_no_conversion(field)?;
                let bind = tmp_binding(i);
                let ty = &field.ty;
                const_size_binding_expr(&bind, ty, fmt)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let fields_size = if field_sizes.is_empty() {
        quote! { 0 }
    } else {
        quote! { 0 #(+ #field_sizes)* }
    };

    Ok(quote! {
        #ty_ident::#var_ident #var_bindings => {
            #discrim_size + #fields_size
        }
    })
}

fn const_size_binding_expr(bind: &syn::Ident, ty: &syn::Type, fmt: Format) -> Result<TokenStream> {
    const_size_value_expr(quote! { #bind }, ty, fmt)
}

fn const_write_binding_expr(bind: &syn::Ident, ty: &syn::Type, fmt: Format) -> Result<TokenStream> {
    const_write_value_expr(quote! { #bind }, ty, fmt)
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

fn transparent_type(ty: &syn::Type) -> &syn::Type {
    match ty {
        syn::Type::Paren(paren) => transparent_type(&paren.elem),
        syn::Type::Group(group) => transparent_type(&group.elem),
        _ => ty,
    }
}

fn primitive_size_fn(ty: &syn::Type) -> Option<TokenStream> {
    let ty = transparent_type(ty);
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
        "i128" => Some(quote! { size_i128 }),
        "u128" => Some(quote! { size_u128 }),
        "i16" => Some(quote! { size_i16 }),
        "u16" => Some(quote! { size_u16 }),
        "bool" => Some(quote! { size_bool }),
        "f32" => Some(quote! { size_f32 }),
        "f64" => Some(quote! { size_f64 }),
        _ => None,
    }
}

fn primitive_max_size_fn(ty: &syn::Type) -> Option<TokenStream> {
    let ty = transparent_type(ty);
    let path = match ty {
        syn::Type::Path(p) => p,
        _ => return None,
    };

    let ident = path.path.get_ident()?;
    let s = ident.to_string();

    match s.as_str() {
        "i32" => Some(quote! { max_size_i32 }),
        "u32" => Some(quote! { max_size_u32 }),
        "i64" => Some(quote! { max_size_i64 }),
        "u64" => Some(quote! { max_size_u64 }),
        "i128" => Some(quote! { max_size_i128 }),
        "u128" => Some(quote! { max_size_u128 }),
        "i16" => Some(quote! { max_size_i16 }),
        "u16" => Some(quote! { max_size_u16 }),
        "bool" => Some(quote! { max_size_bool }),
        "f32" => Some(quote! { max_size_f32 }),
        "f64" => Some(quote! { max_size_f64 }),
        _ => None,
    }
}

fn primitive_write_method(ty: &syn::Type) -> Option<TokenStream> {
    let ty = transparent_type(ty);
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
        "i128" => Some(quote! { write_i128 }),
        "u128" => Some(quote! { write_u128 }),
        "i16" => Some(quote! { write_i16 }),
        "u16" => Some(quote! { write_u16 }),
        "bool" => Some(quote! { write_bool }),
        "f32" => Some(quote! { write_f32 }),
        "f64" => Some(quote! { write_f64 }),
        _ => None,
    }
}

fn check_supported_const_shape(ty: &syn::Type) -> Result<()> {
    let ty = transparent_type(ty);

    if primitive_size_fn(ty).is_some() || is_u8_array(ty) || is_u8_slice_ref(ty) || is_str_ref(ty) {
        return Ok(());
    }

    if bytesm_stringm_max(ty).is_some() {
        return Ok(());
    }

    if let Some((elem, _)) = vecm_parts(ty) {
        if is_path_ident(elem, "u8") {
            return Ok(());
        }
        return check_supported_const_shape(elem);
    }

    if let Some(inner) = option_inner(ty) {
        return check_supported_const_shape(inner);
    }

    if let Some((ok_ty, err_ty)) = result_inners(ty) {
        check_supported_const_shape(ok_ty)?;
        return check_supported_const_shape(err_ty);
    }

    if let Some(inner) = slice_ref_inner(ty) {
        return check_supported_const_shape(inner);
    }

    if let Some((inner, _)) = array_inner(ty) {
        return check_supported_const_shape(inner);
    }

    if let Some(elems) = tuple_elems(ty) {
        for elem in elems {
            check_supported_const_shape(elem)?;
        }
        return Ok(());
    }

    if let syn::Type::Reference(r) = ty {
        return check_supported_const_shape(&r.elem);
    }

    if matches!(ty, syn::Type::Path(_)) {
        return Ok(());
    }

    err(ty, "ConstWriteXDR does not support this type form.")
}

fn is_path_ident(ty: &syn::Type, name: &str) -> bool {
    let ty = transparent_type(ty);
    if let syn::Type::Path(p) = ty
        && let Some(ident) = p.path.get_ident()
    {
        return ident == name;
    }
    false
}

/// Matches `VecM<T>` / `VecM<T, MAX>` by last path segment, returning the
/// element type and the `MAX` argument tokens when explicitly provided.
fn vecm_parts(ty: &syn::Type) -> Option<(&syn::Type, Option<TokenStream>)> {
    let seg = path_segment(ty, "VecM")?;
    let args = angle_bracketed_args(seg)?;
    let mut iter = args.args.iter();
    let elem = match iter.next()? {
        syn::GenericArgument::Type(t) => transparent_type(t),
        _ => return None,
    };
    let max = match iter.next() {
        None => None,
        // A const argument that is a bare path (e.g. a const generic `N`)
        // parses ambiguously as a Type; both forms splice back verbatim.
        Some(arg @ (syn::GenericArgument::Const(_) | syn::GenericArgument::Type(_))) => {
            Some(quote! { #arg })
        }
        Some(_) => return None,
    };
    if iter.next().is_some() {
        return None;
    }
    Some((elem, max))
}

/// Matches `BytesM` / `StringM` (with or without an explicit `MAX` argument)
/// by last path segment, returning the `MAX` argument tokens when provided.
fn bytesm_stringm_max(ty: &syn::Type) -> Option<Option<TokenStream>> {
    let seg = path_segment(ty, "BytesM").or_else(|| path_segment(ty, "StringM"))?;
    let args = match &seg.arguments {
        syn::PathArguments::None => return Some(None),
        syn::PathArguments::AngleBracketed(args) => args,
        syn::PathArguments::Parenthesized(_) => return None,
    };
    let mut iter = args.args.iter();
    let max = match iter.next() {
        None => None,
        Some(arg @ (syn::GenericArgument::Const(_) | syn::GenericArgument::Type(_))) => {
            Some(quote! { #arg })
        }
        Some(_) => return None,
    };
    if iter.next().is_some() {
        return None;
    }
    Some(max)
}

/// Generates a length guard against an explicit `MAX` bound, if one was
/// provided. Expects a `slice` binding to be in scope in the generated code.
fn max_len_guard(max: &Option<TokenStream>) -> TokenStream {
    match max {
        Some(max) => quote! {
            ::core::assert!(slice.len() <= (#max) as usize, "length exceeds MAX");
        },
        None => quote! {},
    }
}

fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let seg = path_segment(ty, "Option")?;
    let args = angle_bracketed_args(seg)?;
    let mut types = args.args.iter().filter_map(|arg| {
        if let syn::GenericArgument::Type(ty) = arg {
            Some(ty)
        } else {
            None
        }
    });
    let inner = types.next()?;
    if types.next().is_some() {
        return None;
    }
    Some(inner)
}

fn result_inners(ty: &syn::Type) -> Option<(&syn::Type, &syn::Type)> {
    let seg = path_segment(ty, "Result")?;
    let args = angle_bracketed_args(seg)?;
    let mut types = args.args.iter().filter_map(|arg| {
        if let syn::GenericArgument::Type(ty) = arg {
            Some(ty)
        } else {
            None
        }
    });
    let ok_ty = types.next()?;
    let err_ty = types.next()?;
    if types.next().is_some() {
        return None;
    }
    Some((ok_ty, err_ty))
}

fn slice_ref_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let ty = transparent_type(ty);
    if let syn::Type::Reference(r) = ty
        && let syn::Type::Slice(s) = transparent_type(r.elem.as_ref())
    {
        return Some(transparent_type(s.elem.as_ref()));
    }
    None
}

fn array_inner(ty: &syn::Type) -> Option<(&syn::Type, &syn::Expr)> {
    let ty = transparent_type(ty);
    if let syn::Type::Array(arr) = ty {
        return Some((transparent_type(arr.elem.as_ref()), &arr.len));
    }
    None
}

fn tuple_elems(ty: &syn::Type) -> Option<&syn::punctuated::Punctuated<syn::Type, syn::Token![,]>> {
    let ty = transparent_type(ty);
    if let syn::Type::Tuple(tuple) = ty {
        return Some(&tuple.elems);
    }
    None
}

fn tuple_elem_ref(expr_ref: &TokenStream, index: usize) -> TokenStream {
    let index = syn::Index::from(index);
    quote! { &(*#expr_ref).#index }
}

fn path_segment<'a>(ty: &'a syn::Type, name: &str) -> Option<&'a syn::PathSegment> {
    let ty = transparent_type(ty);
    let path = match ty {
        syn::Type::Path(path) if path.qself.is_none() => path,
        _ => return None,
    };

    let seg = path.path.segments.last()?;
    if seg.ident == name { Some(seg) } else { None }
}

fn angle_bracketed_args(seg: &syn::PathSegment) -> Option<&syn::AngleBracketedGenericArguments> {
    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
        Some(args)
    } else {
        None
    }
}

fn is_u8_array(ty: &syn::Type) -> bool {
    let ty = transparent_type(ty);
    if let syn::Type::Array(arr) = ty {
        return is_path_ident(arr.elem.as_ref(), "u8");
    }
    false
}

fn is_u8_slice_ref(ty: &syn::Type) -> bool {
    let ty = transparent_type(ty);
    if let syn::Type::Reference(r) = ty
        && let syn::Type::Slice(s) = transparent_type(r.elem.as_ref())
    {
        return is_path_ident(s.elem.as_ref(), "u8");
    }
    false
}

fn is_str_ref(ty: &syn::Type) -> bool {
    let ty = transparent_type(ty);
    if let syn::Type::Reference(r) = ty {
        return is_path_ident(r.elem.as_ref(), "str");
    }
    false
}

fn is_phantom_data(ty: &syn::Type) -> bool {
    path_segment(ty, "PhantomData").is_some()
}

fn is_infallible(ty: &syn::Type) -> bool {
    path_segment(ty, "Infallible").is_some()
}
