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

    Ok(quote! {
        impl #impl_generics #ty_ident #ty_generics #where_clause {
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
    const_size_value_expr(quote! { &self.#fa }, &field.ty, fmt)
}

fn const_size_value_expr(
    expr_ref: TokenStream,
    ty: &syn::Type,
    fmt: Format,
) -> Result<TokenStream> {
    check_supported_const_shape(ty)?;

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

    if is_i128(ty) {
        return Ok(quote! {
            (#mod_path::size_i64((*#expr_ref >> 64) as i64)
             + #mod_path::size_u64((*#expr_ref & 0xffffffffffffffff) as u64))
        });
    }
    if is_u128(ty) {
        return Ok(quote! {
            (#mod_path::size_u64((*#expr_ref >> 64) as u64)
             + #mod_path::size_u64((*#expr_ref & 0xffffffffffffffff) as u64))
        });
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

    if let Some(inner) = option_inner(ty) {
        let inner_size = const_size_value_expr(quote! { inner }, inner, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Some(inner) => #mod_path::size_i32(1) + #inner_size,
                None => #mod_path::size_i32(0),
            }
        });
    }

    if let Some((ok_ty, err_ty)) = result_inners(ty) {
        let ok_size = const_size_value_expr(quote! { inner }, ok_ty, fmt)?;
        let err_size = const_size_value_expr(quote! { inner }, err_ty, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Ok(inner) => #mod_path::size_i32(0) + #ok_size,
                Err(inner) => #mod_path::size_i32(1) + #err_size,
            }
        });
    }

    if let Some(inner) = slice_ref_inner(ty) {
        let inner_size = const_size_value_expr(quote! { &slice[i] }, inner, fmt)?;
        return Ok(quote! {
            {
                let slice = *#expr_ref;
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

    let method = match fmt {
        Format::Cxs => quote! { const_write_cxs },
        Format::Txs => quote! { const_write_txs },
    };

    if let Some(writer_method) = primitive_write_method(ty) {
        return Ok(quote! { w.#writer_method(*#expr_ref); });
    }

    if is_i128(ty) {
        return Ok(quote! {
            w.write_i64((*#expr_ref >> 64) as i64);
            w.write_u64((*#expr_ref & 0xffffffffffffffff) as u64);
        });
    }
    if is_u128(ty) {
        return Ok(quote! {
            w.write_u64((*#expr_ref >> 64) as u64);
            w.write_u64((*#expr_ref & 0xffffffffffffffff) as u64);
        });
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

    if let Some(inner) = option_inner(ty) {
        let inner_write = const_write_value_expr(quote! { inner }, inner, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Some(inner) => {
                    w.write_i32(1);
                    #inner_write
                }
                None => w.write_i32(0),
            }
        });
    }

    if let Some((ok_ty, err_ty)) = result_inners(ty) {
        let ok_write = const_write_value_expr(quote! { inner }, ok_ty, fmt)?;
        let err_write = const_write_value_expr(quote! { inner }, err_ty, fmt)?;
        return Ok(quote! {
            match #expr_ref {
                Ok(inner) => {
                    w.write_i32(0);
                    #ok_write
                }
                Err(inner) => {
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

    Ok(quote! { (#expr_ref).#method(w); })
}

fn const_write_enum(input: &syn::DeriveInput, item: &syn::DataEnum) -> Result<TokenStream> {
    check_enum_repr(input)?;
    check_variants(input, item)?;

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

    Ok(quote! {
        impl #impl_generics #ty_ident #ty_generics #where_clause {
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
        assert!(toks.contains("match&self.maybe{Some(inner)=>"));
        assert!(toks.contains("match&self.nested{Some(inner)=>"));
        assert!(toks.contains("match&self.choice{Ok(inner)=>"));
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
        assert!(toks.contains("&(*inner).0"));
        assert!(toks.contains("&(*&(*inner).1).0"));
        assert!(!toks.contains("&&inner.0"));
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
            .collect::<Result<Vec<_>>>()?;
        quote! { #(#field_writes)* }
    };

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
            .collect::<Result<Vec<_>>>()?;
        if field_sizes.is_empty() {
            quote! { 0 }
        } else {
            quote! { 0 #(+ #field_sizes)* }
        }
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

fn check_supported_const_shape(ty: &syn::Type) -> Result<()> {
    if primitive_size_fn(ty).is_some()
        || is_i128(ty)
        || is_u128(ty)
        || is_u8_array(ty)
        || is_u8_slice_ref(ty)
        || is_str_ref(ty)
    {
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

    if matches!(ty, syn::Type::Path(_)) {
        return Ok(());
    }

    err(ty, "ConstWriteXDR does not support this type form.")
}

fn is_path_ident(ty: &syn::Type, name: &str) -> bool {
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
    if let syn::Type::Reference(r) = ty
        && let syn::Type::Slice(s) = &*r.elem
    {
        return Some(&s.elem);
    }
    None
}

fn array_inner(ty: &syn::Type) -> Option<(&syn::Type, &syn::Expr)> {
    if let syn::Type::Array(arr) = ty {
        return Some((&arr.elem, &arr.len));
    }
    None
}

fn tuple_elems(ty: &syn::Type) -> Option<&syn::punctuated::Punctuated<syn::Type, syn::Token![,]>> {
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
