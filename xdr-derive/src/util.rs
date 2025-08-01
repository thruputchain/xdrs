use proc_macro2::TokenStream;
use quote::quote;
use syn::{Result, Error};
use syn::spanned::Spanned;

// Produce an error result referencing a particular code span
pub fn err<T, S, M>(item: &S, msg: M) -> Result<T>
where
    S: Spanned,
    M: AsRef<str>,
{
    Err(error(item, msg))
}

// Produce an error result referencing a particular code span
pub fn error<S, M>(item: &S, msg: M) -> Error
where
    S: Spanned,
    M: AsRef<str>,
{
    syn::Error::new(item.span(), msg.as_ref())
}

#[allow(dead_code)]
pub fn err_tok<S, M>(item: &S, msg: M) -> TokenStream
where
    S: Spanned,
    M: AsRef<str>,
{
    syn::Error::new(item.span(), msg.as_ref()).into_compile_error()
}

pub enum FieldAccess<'a> {
    Ident(&'a syn::Ident),
    Index(usize),
}

impl<'a> FieldAccess<'a> {
    pub fn new(index: usize, field: &'a syn::Field) -> Self {
        if let Some(id) = field.ident.as_ref() {
            FieldAccess::Ident(id)
        } else {
            FieldAccess::Index(index)
        }
    }
}

impl<'a> quote::ToTokens for FieldAccess<'a> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match *self {
            FieldAccess::Ident(id) => id.to_tokens(tokens),
            FieldAccess::Index(ix) => {
                let ix = syn::Index::from(ix);
                tokens.extend(quote! { #ix })
            }
        }
    }
}

pub fn token_reduce<T, I, F>(i: I, mut f: F) -> TokenStream
where
    I: IntoIterator<Item = T>,
    F: FnMut(usize, T) -> TokenStream,
{
    TokenStream::from_iter(i.into_iter().enumerate().map(|(i, x)| f(i, x)))
}

pub fn tmp_binding(index: usize) -> syn::Ident {
    quote::format_ident!("_self_{}", index)
}

pub fn modify_for_reader_impl(mut generics: syn::Generics) -> syn::Generics {
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

pub fn find_default_variant(
    input: &syn::DeriveInput,
    item: &syn::DataEnum,
) -> Result<Option<usize>> {
    let variants = &item.variants;
    let n_variants = variants.iter().count();
    let n_explicit = variants.iter().filter(|v| v.discriminant.is_some()).count();
    let n_implicit = n_variants - n_explicit;

    if n_variants > 1 && n_implicit == 1 {
        if let Some(ix) = variants
            .iter()
            .enumerate()
            .find(|(_, v)| {
                v.discriminant.is_none()
                    && v.fields.len() == 1
                    && v.ident.to_string().as_str() == "Default"
            })
            .map(|(i, _)| i)
        {
            Ok(Some(ix))
        } else {
            enum_discrim_err(input)
        }
    } else {
        if n_variants > 1 && !(n_implicit == 0 || n_explicit == 0) {
            enum_discrim_err(input)
        } else {
            Ok(None)
        }
    }
}

fn enum_discrim_err<T>(input: &syn::DeriveInput) -> Result<T> {
    err(
        input,
        "derive XDR: enum discriminants must be all implicit or \
         all explicit, and may optionally also include a default \
         variant (with no explicit discriminant) named \"Default\", \
         which must have one i32 field",
         )
}
