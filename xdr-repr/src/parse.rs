use super::xdr::*;
use syn::parse::*;
use syn::Token;
use crate::alloc::string::ToString;
use alloc::{vec, vec::Vec};

mod kw {
    syn::custom_keyword!(void);
}

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
        input.parse().map(Definition::Const)
            .or_else(|_| input.parse().map(Definition::Type))
    }
}

impl Parse for Declaration {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let _ = input.parse::<kw::void>()?;
        Ok(Declaration::Void)
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

impl Parse for TypeDef {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        /*
        input.parse().map(TypeDef::Def)
          .or_else(|_| 
                parse_aggregate_def(input))
        */
        let d = parse_aggregate_def(input)?;
        let _ = input.parse::<Token![;]>()?;
        Ok(d)
    }
}

fn parse_aggregate_def(input: ParseStream<'_>) -> Result<TypeDef> {
    input.parse::<Token![enum]>()
        .and_then(|_| Ok(TypeDef::Enum(input.parse()?, input.parse()?)))
        .or_else(|_| input.parse::<Token![struct]>()
            .and_then(|_| Ok(TypeDef::Struct(input.parse()?, input.parse()?))))
        /*
            .or_else(|_| input.parse::<Token![union]>()
                .and_then(|_| Ok(TypeDef::Union(input.parse()?, input.parse()?)))))
        */
}

impl Parse for EnumBody {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let content;
        let _ = syn::braced!(content in input);
        let variants: syn::punctuated::Punctuated<_, _> =
            content.parse_terminated(EnumVariant::parse, Token![,])?;
        Ok(EnumBody(variants.into_iter().map(|v| v.0).collect()))
    }
}

struct EnumVariant((Ident, Value));

impl Parse for EnumVariant {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let ident = input.parse()?;
        let _ = input.parse::<Token![=]>()?;
        let value = input.parse()?;
        Ok(EnumVariant((ident, value)))
    }
}

impl Parse for StructBody {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let content;
        let _ = syn::braced!(content in input);
        let fields: syn::punctuated::Punctuated<_, _> =
            content.parse_terminated(Declaration::parse, Token![;])?;
        Ok(StructBody(fields.into_iter().collect()))
    }
}

impl Parse for Value {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse().map(Value::Const)
            .or_else(|_| input.parse().map(Value::Ident))
    }
}

impl Parse for Ident {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        // TODO: allow only legal XDR identifiers
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
