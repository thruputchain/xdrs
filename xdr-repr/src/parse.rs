use super::xdr::*;
use syn::parse::*;
use syn::Token;
use crate::alloc::string::ToString;
use alloc::{vec, vec::Vec};

impl Parse for Specification {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut definitions: Vec<Definition> = vec![];
        while !input.peek(End) {
            definitions.push(input.parse()?);
        }
        Ok(Specification { definitions })
    }
}

impl Parse for Definition {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        Ok(Definition::Const(input.parse()?))
    }
}

impl Parse for ConstDef {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let _ = input.parse::<Token![const]>()?;
        let ident = input.parse()?;
        let _ = input.parse::<Token![=]>()?;
        let constant = input.parse()?;
        let _ = input.parse::<Token![;]>()?;

        Ok(ConstDef { ident, constant })
    }
}

impl Parse for Ident {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        // XXX: allow only legal XDR identifiers
        let ident: syn::Ident = input.parse()?;

        Ok(Ident(ident.to_string()))
    }
}

impl Parse for Constant {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let lit: syn::LitInt = input.parse()?;
        lit.base10_parse::<u32>().map(Constant::Unsigned)
            .or_else(|_| lit.base10_parse::<i32>().map(Constant::Signed))
    }
}
