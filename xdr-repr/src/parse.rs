use super::xdr::*;
use syn::parse::*;
use syn::{Token, token};
use syn::punctuated::Punctuated;
use crate::alloc::string::ToString;
use alloc::{vec, vec::Vec};
use alloc::format;

const KEYWORDS: [&str; 18] = [
   "bool", "case", "const", "default", "double", "quadruple", "enum",
   "float", "hyper", "int", "opaque", "string", "struct", "switch",
   "typedef", "union", "unsigned", "void"
];

fn is_keyword<S: AsRef<str>>(s: S) -> bool {
    KEYWORDS.into_iter().any(|k| s.as_ref() == k)
}

mod kw {
    syn::custom_keyword!(void);
    syn::custom_keyword!(opaque);
    syn::custom_keyword!(string);

    syn::custom_keyword!(unsigned);
    syn::custom_keyword!(int);
    syn::custom_keyword!(hyper);
    syn::custom_keyword!(float);
    syn::custom_keyword!(double);
    syn::custom_keyword!(quadruple);
    syn::custom_keyword!(bool);
}

syn::custom_punctuation!(LeftArrow, <);
syn::custom_punctuation!(RightArrow, >);

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
        if input.parse::<kw::void>().is_ok() {
            Ok(Declaration::Void)
        } else if input.parse::<kw::opaque>().is_ok() {
            let name = input.parse()?;
            if input.peek(token::Bracket) {
                let size = parse_bracketed(input, Value::parse)?;
                Ok(Declaration::FixedOpaque { name, size })
            } else {
                let max = parse_arrowed(input, Value::parse)?;
                Ok(Declaration::VariableOpaque { name, max })
            }
        } else if input.peek3(token::Bracket) {
            let elt_ty = input.parse()?;
            let name = input.parse()?;
            let len = parse_bracketed(input, Value::parse)?;
            Ok(Declaration::FixedArray { name, elt_ty, len })
        } else if input.peek3(LeftArrow) {
            let elt_ty = input.parse()?;
            let name = input.parse()?;
            let max = parse_arrowed(input, Value::parse)?;
            Ok(Declaration::VariableArray { name, elt_ty, max })
        } else {
            let ty = input.parse()?;
            let name = input.parse()?;
            Ok(Declaration::Named { ty, name })
        }
    }
}

fn parse_bracketed<T>(input: ParseStream, parser: fn(ParseStream) -> Result<T>) -> Result<T> {
    let contents;
    let _ = syn::bracketed! { contents in input };
    let values: Punctuated<T, Token![,]> = parse_one_with(&contents, parser)?;
    let value = values.into_iter().next().unwrap();
    Ok(value)
}

fn parse_arrowed<T>(input: ParseStream, parser: fn(ParseStream) -> Result<T>) -> Result<T> {
    let _ = input.parse::<LeftArrow>()?;
    let value = input.call(parser)?;
    let _ = input.parse::<RightArrow>()?;
    Ok(value)
}

impl Parse for TypeSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse::<kw::unsigned>().and_then(|_|
            input.parse::<kw::int>().and(Ok(TypeSpec::Unsigned)).or_else(|_|
                input.parse::<kw::hyper>().and(Ok(TypeSpec::UnsignedHyper))))
            .or_else(|_| input.parse::<kw::int>().and(Ok(TypeSpec::Int)))
            .or_else(|_| input.parse::<kw::hyper>().and(Ok(TypeSpec::Hyper)))
            .or_else(|_| input.parse::<kw::float>().and(Ok(TypeSpec::Float)))
            .or_else(|_| input.parse::<kw::double>().and(Ok(TypeSpec::Double)))
            .or_else(|_| input.parse::<kw::quadruple>().and(Ok(TypeSpec::Quadruple)))
            .or_else(|_| input.parse::<kw::bool>().and(Ok(TypeSpec::Bool)))
            .or_else(|_| input.parse::<Token![enum]>().and_then(|_| input.parse().map(TypeSpec::Enum)))
            .or_else(|_| input.parse::<Token![struct]>().and_then(|_| input.parse().map(TypeSpec::Struct)))
            // .or_else(|_| input.parse::<Token![union]>().and_then(|_| input.parse().map(TypeSpec::Union)))
            .or_else(|_| input.parse().map(TypeSpec::Ident))
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
            content.parse_terminated(parse_enum_variant, Token![,])?;
        Ok(EnumBody(variants.into_iter().collect()))
    }
}

pub fn parse_one_with<'a, T, P>(
        input: ParseStream<'a>,
        parser: fn(ParseStream<'a>) -> Result<T>,
) -> Result<syn::punctuated::Punctuated<T, P>>
where
    P: Parse,
{
    let mut punctuated = Punctuated::new();

    let value = parser(input)?;
    punctuated.push_value(value);

    Ok(punctuated)
}

fn parse_enum_variant(input: ParseStream<'_>) -> Result<(Ident, Value)> {
    let ident = input.parse()?;
    let _ = input.parse::<Token![=]>()?;
    let value = input.parse()?;
    Ok((ident, value))
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
        let ident: syn::Ident = input.parse()?;
        let ident = ident.to_string();

        if is_keyword(&ident) {
            return Err(input.error(format!("Found keyword '{ident}' when expecting identifier")));
        }

        Ok(Ident(ident))
    }
}

impl Parse for Constant {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let lit: syn::LitInt = input.parse()?;
        lit.base10_parse::<u32>().map(Constant::Unsigned)
            .or_else(|_| lit.base10_parse::<i32>().map(Constant::Signed))
    }
}
