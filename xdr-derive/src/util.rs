use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;
use syn::spanned::Spanned;

// Produce an error referencing a particular code span
pub fn err<T, S, M>(spanned: &S, msg: M) -> Result<T>
where
    S: Spanned,
    M: AsRef<str>,
{
    Err(syn::Error::new(spanned.span(), msg.as_ref()))
}

pub enum FieldAccess<'a> {
    Ident(&'a syn::Ident),
    Index(usize),
}

impl<'a> From<(&'a syn::Field, usize)> for FieldAccess<'a> {
    fn from(f: (&'a syn::Field, usize)) -> Self {
        if let Some(id) = f.0.ident.as_ref() {
            FieldAccess::Ident(id)
        } else {
            FieldAccess::Index(f.1)
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
