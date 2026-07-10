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

    /// When set, skip the inherent `const fn` encoders and emit only the
    /// `ConstWriteXDR` trait impl with direct (non-`const`) bodies. Needed for
    /// types whose fields (e.g. `StringM`) have no inherent const encoders.
    pub runtime_only: bool,

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

    let impls = const_write_data(input, &attrs, &krate)?;

    Ok(quote! {
        const _: () = {
            use #krate::cxs;
            use #krate::txs;

            #impls
        };
    })
}

fn const_write_data(
    input: &syn::DeriveInput,
    attrs: &Attrs,
    krate: &syn::Path,
) -> Result<TokenStream> {
    let has_type_params = input
        .generics
        .params
        .iter()
        .any(|p| matches!(p, syn::GenericParam::Type(_)));

    match &input.data {
        syn::Data::Struct(item) => const_write_struct(input, item, attrs, krate, has_type_params),
        syn::Data::Enum(item) => const_write_enum(input, item, attrs, krate, has_type_params),
        _ => err(
            input,
            "ConstWriteXDR can only be derived for structs and enums",
        ),
    }
}

fn const_write_struct(
    input: &syn::DeriveInput,
    item: &syn::DataStruct,
    attrs: &Attrs,
    krate: &syn::Path,
    has_type_params: bool,
) -> Result<TokenStream> {
    let emit_inherent = !has_type_params && !attrs.runtime_only;
    let inherent = if emit_inherent {
        inherent_struct_impl(input, item)?
    } else {
        quote! {}
    };
    let trait_impl = trait_impl_struct(input, item, krate, !emit_inherent)?;

    Ok(quote! {
        #inherent
        #trait_impl
    })
}

fn inherent_struct_impl(input: &syn::DeriveInput, item: &syn::DataStruct) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Trait-only field types have no inherent const encoders to call.
    for f in &item.fields {
        check_field_has_inherent_const_encoder(&f.ty)?;
    }

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
        impl #impl_generics #ty_ident #ty_generics #where_clause {
            pub const fn const_write_cxs(&self, w: &mut cxs::Cursor<'_>) {
                #(#cxs_fields)*
            }

            pub const fn const_write_txs(&self, w: &mut txs::Cursor<'_>) {
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

fn const_write_enum(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    attrs: &Attrs,
    krate: &syn::Path,
    has_type_params: bool,
) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item, has_xdr_discrim_type(input))?;

    let emit_inherent = !has_type_params && !attrs.runtime_only;
    let inherent = if emit_inherent {
        inherent_enum_impl(input, item)?
    } else {
        quote! {}
    };
    let trait_impl = trait_impl_enum(input, item, krate, !emit_inherent)?;

    Ok(quote! {
        #inherent
        #trait_impl
    })
}

fn inherent_enum_impl(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Trait-only field types have no inherent const encoders to call.
    for v in &item.variants {
        for f in &v.fields {
            check_field_has_inherent_const_encoder(&f.ty)?;
        }
    }

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

    Ok(quote! {
        impl #impl_generics #ty_ident #ty_generics #where_clause {
            pub const fn const_write_cxs(&self, w: &mut cxs::Cursor<'_>) {
                match self {
                    #(#cxs_arms)*
                }
            }

            pub const fn const_write_txs(&self, w: &mut txs::Cursor<'_>) {
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

// -- ConstWriteXDR trait impl emission --

impl Format {
    /// Associated const name on `ConstWriteXDR` for this format's worst-case size.
    fn max_const(self) -> TokenStream {
        match self {
            Format::Cxs => quote! { MAX_CXS_LEN },
            Format::Txs => quote! { MAX_TXS_LEN },
        }
    }

    /// Trait write method for this format.
    fn write_method(self) -> TokenStream {
        match self {
            Format::Cxs => quote! { write_cxs },
            Format::Txs => quote! { write_txs },
        }
    }

    /// Trait size method for this format.
    fn size_method(self) -> TokenStream {
        match self {
            Format::Cxs => quote! { cxs_size },
            Format::Txs => quote! { txs_size },
        }
    }

    /// Fully-qualified `size_i32` const fn path for this format.
    fn size_i32_path(self, krate: &syn::Path) -> TokenStream {
        match self {
            Format::Cxs => quote! { #krate::cxs::const_writer::size_i32 },
            Format::Txs => quote! { #krate::txs::const_writer::size_i32 },
        }
    }

    /// Cursor type for this format.
    fn cursor_ty(self, krate: &syn::Path) -> TokenStream {
        match self {
            Format::Cxs => quote! { #krate::cxs::Cursor<'_> },
            Format::Txs => quote! { #krate::txs::Cursor<'_> },
        }
    }
}

/// Clone `generics` and append a `#T: #krate::ConstWriteXDR` where-predicate
/// for every type parameter. Used so the trait impl compiles when the type has
/// generic parameters, without requiring the user to write struct-level bounds
/// (serde-style synthesized bounds).
fn synthesize_trait_generics(generics: &syn::Generics, krate: &syn::Path) -> syn::Generics {
    let mut g = generics.clone();
    let extra: Vec<syn::WherePredicate> = g
        .params
        .iter()
        .filter_map(|p| match p {
            syn::GenericParam::Type(tp) => {
                let ident = &tp.ident;
                Some(syn::parse_quote!(#ident: #krate::ConstWriteXDR))
            }
            _ => None,
        })
        .collect();
    if !extra.is_empty() {
        let wc = g.make_where_clause();
        for pred in extra {
            wc.predicates.push(pred);
        }
    }
    g
}

fn trait_impl_struct(
    input: &syn::DeriveInput,
    item: &syn::DataStruct,
    krate: &syn::Path,
    direct_bodies: bool,
) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let trait_gen = synthesize_trait_generics(&input.generics, krate);
    let (impl_generics, ty_generics, where_clause) = trait_gen.split_for_impl();

    let max_cxs = struct_max_fold(item, krate, Format::Cxs)?;
    let max_txs = struct_max_fold(item, krate, Format::Txs)?;

    let (write_cxs_body, write_txs_body, cxs_size_body, txs_size_body) = if direct_bodies {
        (
            struct_trait_write_body(item, krate, Format::Cxs)?,
            struct_trait_write_body(item, krate, Format::Txs)?,
            struct_trait_size_body(item, krate, Format::Cxs)?,
            struct_trait_size_body(item, krate, Format::Txs)?,
        )
    } else {
        (
            quote! { self.const_write_cxs(w) },
            quote! { self.const_write_txs(w) },
            quote! { self.const_cxs_size() },
            quote! { self.const_txs_size() },
        )
    };

    let cxs_cursor = Format::Cxs.cursor_ty(krate);
    let txs_cursor = Format::Txs.cursor_ty(krate);

    Ok(quote! {
        impl #impl_generics #krate::ConstWriteXDR for #ty_ident #ty_generics #where_clause {
            const MAX_CXS_LEN: ::core::option::Option<usize> = #max_cxs;
            const MAX_TXS_LEN: ::core::option::Option<usize> = #max_txs;

            fn write_cxs(&self, w: &mut #cxs_cursor) { #write_cxs_body }
            fn write_txs(&self, w: &mut #txs_cursor) { #write_txs_body }
            fn cxs_size(&self) -> usize { #cxs_size_body }
            fn txs_size(&self) -> usize { #txs_size_body }
        }
    })
}

/// `add`-fold of every field's worst-case size, starting from `Some(0)`.
fn struct_max_fold(item: &syn::DataStruct, krate: &syn::Path, fmt: Format) -> Result<TokenStream> {
    let max_const = fmt.max_const();
    let adds = item
        .fields
        .iter()
        .map(|f| {
            let attrs = Attrs::from_field(f)?;
            attrs.check_no_conversion(f)?;
            let fty = &f.ty;
            Ok(quote! {
                m = #krate::encoded_max::add(m, <#fty as #krate::ConstWriteXDR>::#max_const);
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! {
        {
            let mut m = ::core::option::Option::Some(0usize);
            #(#adds)*
            m
        }
    })
}

/// Direct per-field trait write calls for a struct (runtime_only path).
fn struct_trait_write_body(
    item: &syn::DataStruct,
    krate: &syn::Path,
    fmt: Format,
) -> Result<TokenStream> {
    let method = fmt.write_method();
    let writes = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let attrs = Attrs::from_field(f)?;
            attrs.check_no_conversion(f)?;
            let fa = FieldAccess::new(i, f);
            let fty = &f.ty;
            Ok(quote! {
                <#fty as #krate::ConstWriteXDR>::#method(&self.#fa, w);
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! { #(#writes)* })
}

/// Direct per-field trait size sum for a struct (runtime_only path).
fn struct_trait_size_body(
    item: &syn::DataStruct,
    krate: &syn::Path,
    fmt: Format,
) -> Result<TokenStream> {
    let method = fmt.size_method();
    let sizes = item
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let attrs = Attrs::from_field(f)?;
            attrs.check_no_conversion(f)?;
            let fa = FieldAccess::new(i, f);
            let fty = &f.ty;
            Ok(quote! {
                <#fty as #krate::ConstWriteXDR>::#method(&self.#fa)
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! { 0 #(+ #sizes)* })
}

fn trait_impl_enum(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    krate: &syn::Path,
    direct_bodies: bool,
) -> Result<TokenStream> {
    let ty_ident = &input.ident;
    let trait_gen = synthesize_trait_generics(&input.generics, krate);
    let (impl_generics, ty_generics, where_clause) = trait_gen.split_for_impl();

    let max_cxs = enum_max_fold(item, krate, Format::Cxs)?;
    let max_txs = enum_max_fold(item, krate, Format::Txs)?;

    let (write_cxs_body, write_txs_body, cxs_size_body, txs_size_body) = if direct_bodies {
        (
            enum_trait_write_body(input, item, krate, Format::Cxs)?,
            enum_trait_write_body(input, item, krate, Format::Txs)?,
            enum_trait_size_body(input, item, krate, Format::Cxs)?,
            enum_trait_size_body(input, item, krate, Format::Txs)?,
        )
    } else {
        (
            quote! { self.const_write_cxs(w) },
            quote! { self.const_write_txs(w) },
            quote! { self.const_cxs_size() },
            quote! { self.const_txs_size() },
        )
    };

    let cxs_cursor = Format::Cxs.cursor_ty(krate);
    let txs_cursor = Format::Txs.cursor_ty(krate);

    Ok(quote! {
        impl #impl_generics #krate::ConstWriteXDR for #ty_ident #ty_generics #where_clause {
            const MAX_CXS_LEN: ::core::option::Option<usize> = #max_cxs;
            const MAX_TXS_LEN: ::core::option::Option<usize> = #max_txs;

            fn write_cxs(&self, w: &mut #cxs_cursor) { #write_cxs_body }
            fn write_txs(&self, w: &mut #txs_cursor) { #write_txs_body }
            fn cxs_size(&self) -> usize { #cxs_size_body }
            fn txs_size(&self) -> usize { #txs_size_body }
        }
    })
}

/// `max`-fold over variants of `add(disc_max, payload_fold)`; `Some(0)` if empty.
fn enum_max_fold(item: &syn::DataEnum, krate: &syn::Path, fmt: Format) -> Result<TokenStream> {
    let variants = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| variant_max(i, v, krate, fmt))
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! {
        {
            let mut acc = ::core::option::Option::Some(0usize);
            #( acc = #krate::encoded_max::max(acc, #variants); )*
            acc
        }
    })
}

fn variant_max(
    index: usize,
    v: &syn::Variant,
    krate: &syn::Path,
    fmt: Format,
) -> Result<TokenStream> {
    let is_default_arm = variant_is_default(v);
    let max_const = fmt.max_const();
    let size_i32 = fmt.size_i32_path(krate);

    let disc_max = if let Some((_, d)) = v.discriminant.as_ref() {
        quote! { ::core::option::Option::Some(#size_i32(#d)) }
    } else if is_default_arm {
        match fmt {
            Format::Cxs => quote! { ::core::option::Option::Some(5usize) },
            Format::Txs => quote! { ::core::option::Option::Some(4usize) },
        }
    } else {
        let idx = index as i32;
        quote! { ::core::option::Option::Some(#size_i32(#idx)) }
    };

    let adds = v
        .fields
        .iter()
        .enumerate()
        .filter(|(i, _)| !(is_default_arm && *i == 0))
        .map(|(_, f)| {
            let attrs = Attrs::from_field(f)?;
            attrs.check_no_conversion(f)?;
            let fty = &f.ty;
            Ok(quote! {
                p = #krate::encoded_max::add(p, <#fty as #krate::ConstWriteXDR>::#max_const);
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        {
            let mut p = ::core::option::Option::Some(0usize);
            #(#adds)*
            #krate::encoded_max::add(#disc_max, p)
        }
    })
}

/// Direct `match self` write body for an enum (runtime_only path).
fn enum_trait_write_body(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    krate: &syn::Path,
    fmt: Format,
) -> Result<TokenStream> {
    let arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| enum_trait_write_arm(i, v, input, krate, fmt))
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! {
        match self {
            #(#arms)*
        }
    })
}

fn enum_trait_write_arm(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    krate: &syn::Path,
    fmt: Format,
) -> Result<TokenStream> {
    let is_default_arm = variant_is_default(v);
    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let var_bindings = const_var_bindings(&v.fields);
    let method = fmt.write_method();

    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        quote! { #d }
    } else if is_default_arm {
        quote! { *_self_0 }
    } else {
        quote! { #variant_idx }
    };

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
                let fty = &field.ty;
                Ok(quote! {
                    <#fty as #krate::ConstWriteXDR>::#method(#bind, w);
                })
            }
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        #ty_ident::#var_ident #var_bindings => {
            w.write_i32(#discriminant);
            #(#field_writes)*
        }
    })
}

/// Direct `match self` size body for an enum (runtime_only path).
fn enum_trait_size_body(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
    krate: &syn::Path,
    fmt: Format,
) -> Result<TokenStream> {
    let arms = item
        .variants
        .iter()
        .enumerate()
        .map(|(i, v)| enum_trait_size_arm(i, v, input, krate, fmt))
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! {
        match self {
            #(#arms)*
        }
    })
}

fn enum_trait_size_arm(
    index: usize,
    v: &syn::Variant,
    input: &syn::DeriveInput,
    krate: &syn::Path,
    fmt: Format,
) -> Result<TokenStream> {
    let is_default_arm = variant_is_default(v);
    let var_ident = &v.ident;
    let ty_ident = &input.ident;
    let var_bindings = const_var_bindings(&v.fields);
    let method = fmt.size_method();
    let size_i32 = fmt.size_i32_path(krate);

    let variant_idx = index as i32;
    let discriminant = if let Some((_, d)) = v.discriminant.as_ref() {
        quote! { #d }
    } else if is_default_arm {
        quote! { *_self_0 }
    } else {
        quote! { #variant_idx }
    };

    let discrim_size = quote! { #size_i32(#discriminant) };

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
                let fty = &field.ty;
                Ok(quote! {
                    <#fty as #krate::ConstWriteXDR>::#method(#bind)
                })
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
    fn type_generics_get_trait_impl_only() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct Bad<T> {
                value: Option<T>,
            }
        };

        let toks = const_write_xdr(&input).unwrap().to_string();
        // Trait impl is emitted …
        assert!(
            toks.contains("ConstWriteXDR for"),
            "expected trait impl in: {toks}"
        );
        // … but no inherent const fns.
        assert!(
            !toks.contains("const_write_cxs"),
            "unexpected inherent const fn in: {toks}"
        );
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
    fn trait_only_field_type_errors_targeted() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct HasString {
                name: StringM<16>,
                id: u32,
            }
        };
        let err = const_write_xdr(&input).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("StringM")
                && msg.contains("only the `ConstWriteXDR` trait impl")
                && msg.contains("runtime_only"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn trait_only_field_type_ok_with_runtime_only() {
        let input: syn::DeriveInput = syn::parse_quote! {
            #[xdrs(runtime_only)]
            struct HasString {
                name: StringM<16>,
                id: u32,
            }
        };
        // runtime_only emits only the trait impl (no inherent const fns), so
        // trait-only field types are fine.
        let toks = const_write_xdr(&input).unwrap().to_string();
        assert!(toks.contains("ConstWriteXDR for"));
        assert!(!toks.contains("const_write_cxs"));
    }

    #[test]
    fn trait_only_field_type_errors_through_wrappers() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct HasBoxedFoo {
                inner: Option<xdr_common::Box<Foo>>,
            }
        };
        let err = const_write_xdr(&input).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Box") && msg.contains("runtime_only"),
            "unexpected error message: {msg}"
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

/// Container / alloc type names whose `ConstWriteXDR` impl provides ONLY the
/// trait (no inherent `const fn` encoders). A field of one of these types in a
/// non-`runtime_only` derive would compile the named-leaf fallback down to a
/// call to a nonexistent inherent method (`const_write_cxs` / `const_cxs_size`),
/// producing an opaque "no method named …" error. We pre-scan for them and
/// emit a targeted error instead.
///
/// Matching is by the path's LAST segment ident — the derive's established
/// convention (the same way `Option`/`Result` are recognized). It can
/// false-positive on a user type coincidentally named e.g. `Vec`; see the
/// hygiene section of the derive docs.
const TRAIT_ONLY_CONST_TYPES: &[&str] = &["VecM", "BytesM", "StringM", "Box", "Vec", "String"];

/// Pre-scan a field type (used only on the INHERENT-emission path) and reject
/// types that provide only the `ConstWriteXDR` trait impl. Recurses through the
/// same syntactically-recognized wrappers as the write/size dispatch
/// (Option/Result/tuples/arrays/slices/references) so e.g. `Option<StringM<16>>`
/// is caught too.
fn check_field_has_inherent_const_encoder(ty: &syn::Type) -> Result<()> {
    let ty = transparent_type(ty);

    // Leaf shapes the inherent dispatch handles directly (never a trait call).
    if primitive_size_fn(ty).is_some()
        || is_u8_array(ty)
        || is_u8_slice_ref(ty)
        || is_str_ref(ty)
        || is_phantom_data(ty)
        || is_infallible(ty)
    {
        return Ok(());
    }

    if let Some(inner) = option_inner(ty) {
        return check_field_has_inherent_const_encoder(inner);
    }
    if let Some((ok_ty, err_ty)) = result_inners(ty) {
        check_field_has_inherent_const_encoder(ok_ty)?;
        return check_field_has_inherent_const_encoder(err_ty);
    }
    if let Some(inner) = slice_ref_inner(ty) {
        return check_field_has_inherent_const_encoder(inner);
    }
    if let Some((inner, _)) = array_inner(ty) {
        return check_field_has_inherent_const_encoder(inner);
    }
    if let Some(elems) = tuple_elems(ty) {
        for elem in elems {
            check_field_has_inherent_const_encoder(elem)?;
        }
        return Ok(());
    }
    if let syn::Type::Reference(r) = ty {
        return check_field_has_inherent_const_encoder(&r.elem);
    }

    if let syn::Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
    {
        let name = seg.ident.to_string();
        if let Some(matched) = TRAIT_ONLY_CONST_TYPES.iter().find(|n| **n == name) {
            return err(
                ty,
                format!(
                    "`{matched}` provides only the `ConstWriteXDR` trait impl \
                     (no inherent const encoders); add `#[xdrs(runtime_only)]` to \
                     the deriving type to emit only the trait impl"
                ),
            );
        }
    }

    Ok(())
}
