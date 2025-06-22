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
