use super::xdr::*;
#[allow(unused)]
use quote::{quote, ToTokens, TokenStreamExt};
use proc_macro2::TokenStream;

impl ToTokens for Specification {
    fn to_tokens(&self, output: &mut TokenStream) {
        let defs = &self.definitions;

        output.extend(quote! {
            specification(vec![#(#defs),*])
        });
    }
}

// pub enum Definition {
//     Type(TypeDef),
//     Const(ConstDef),
// }

impl ToTokens for Definition {
    fn to_tokens(&self, output: &mut TokenStream) {
        output.extend(match self {
            Definition::Type(type_def) => quote! {
                def_type(#type_def)
            },
            Definition::Const(ConstDef { ident: Ident(name), constant }) => quote! {
                def_const(#name, #constant)
            },
        })
    }
}

// pub enum TypeDef {
//     Def(Declaration),
//     Enum(Ident, EnumBody),
//     Struct(Ident, StructBody),
//     Union(Ident, UnionBody),
// }

impl ToTokens for TypeDef {
    fn to_tokens(&self, output: &mut TokenStream) {
        output.extend(match self {
            TypeDef::Def(decl) => quote! {
                def(#decl)
            },
            TypeDef::Enum(Ident(name), EnumBody(variants)) => {
                let variants = variants.iter().map(|(ident, value)|
                    quote! { (#ident, #value) });

                quote! {
                    enum_def(#name, vec![#(#variants),*])
                }
            },
            TypeDef::Struct(Ident(name), StructBody(fields)) => quote! {
                struct_def(#name, vec![#(#fields),*])
            },
            TypeDef::Union(Ident(name), UnionBody {
                discriminant_decl,
                case_specs,
                default_decl,
            }) => {
                let (discrim_name, discrim_type) = discriminant_info(discriminant_decl);
                let default = if let Some(decl) = default_decl {
                    quote! { Some(#decl) }
                } else {
                    quote! { None }
                };
                quote! {
                    union_def(#name, #discrim_name, #discrim_type, vec![#(#case_specs),*], #default)
                }
            },
        })
    }
}

fn discriminant_info(decl: &Declaration) -> (&str, &TypeSpec) {
    if let Declaration::Named { name: Ident(name), ty } = decl {
        (name, ty)
    } else {
        panic!("Discriminant declaration must use a typedef: {:?}", decl);
    }
}

impl ToTokens for TypeSpec {
    fn to_tokens(&self, output: &mut TokenStream) {
        output.extend(match self {
            TypeSpec::Int => quote! { XDR_I32 },
            TypeSpec::Unsigned => quote! { XDR_U32 },
            TypeSpec::Hyper => quote! { XDR_I64 },
            TypeSpec::UnsignedHyper => quote! { XDR_U32 },
            TypeSpec::Float => quote! { XDR_FLOAT },
            TypeSpec::Double => quote! { XDR_DOUBLE },
            TypeSpec::Quadruple => quote! { XDR_QUAD },
            TypeSpec::Bool => quote! { XDR_BOOL },
            TypeSpec::Struct(StructBody(fields)) => quote! {
                struct_type(vec![#(#fields),*])
            },
            TypeSpec::Enum(EnumBody(variants)) => {
                let variants = variants.iter().map(|(ident, value)|
                    quote! { (#ident, #value) });

                quote! {
                    enum_type(vec![#(#variants),*])
                }
            },
            // TypeSpec::Union(Box<UnionBody>),
            TypeSpec::Ident(Ident(name)) => quote! {
                named_type(#name)
            },
            _ => todo!()
        });
    }
}

// pub struct CaseSpec {
//     pub cases: Vec<Value>, // must be non-empty
//     pub decl: Declaration,
// }
impl ToTokens for CaseSpec {
    fn to_tokens(&self, output: &mut TokenStream) {
        let CaseSpec { cases, decl } = self;

        output.extend(if cases.len() == 1 {
            let case = &cases[0];
            quote! {
                case_spec(#case, #decl)
            }
        } else {
            quote! {
                CaseSpec {
                    cases: vec![#(#cases),*],
                    decl: #decl,
                }
            }
        });
    }
}

impl ToTokens for Constant {
    fn to_tokens(&self, output: &mut TokenStream) {
        let n = self.0;

        output.extend(quote! {
            constant(#n)
        });
    }
}

impl ToTokens for Ident {
    fn to_tokens(&self, output: &mut TokenStream) {
        let name = self.0.as_str();

        output.extend(quote! {
            ident(#name)
        });
    }
}

// pub enum Value {
//     Const(Constant),
//     Ident(Ident),
// }
impl ToTokens for Value {
    fn to_tokens(&self, output: &mut TokenStream) {
        output.extend(match self {
            Value::Const(Constant(n)) => quote! {
                constant(#n)
            },
            Value::Ident(Ident(name)) => quote! {
                name_ref(#name)
            },
        });
    }
}

/*
pub enum Declaration {
    Named {
        name: Ident,
        ty: TypeSpec,
    },
    FixedArray {
        name: Ident,
        elt_ty: TypeSpec,
        len: Value,
    },
    FixedOpaque {
        name: Ident,
        size: Value,
    },
    VariableArray {
        name: Ident,
        elt_ty: TypeSpec,
        max: Value,
    },
    VariableOpaque {
        name: Ident,
        max: Value,
    },
    VariableString {
        name: Ident,
        max: Value,
    },
    Optional {
        name: Ident,
        opt_ty: TypeSpec,
    },
    Void,
}
*/
impl ToTokens for Declaration {
    fn to_tokens(&self, output: &mut TokenStream) {
        output.extend(match self {
            Declaration::Named { name: Ident(name), ty } => quote! {
                declare_type(#name, #ty)
            },
            Declaration::FixedArray { name: Ident(name), elt_ty, len } => quote! {
                declare_fixed_array(#name, #elt_ty, #len)
            },
            Declaration::FixedOpaque { name: Ident(name), size } =>
                match size {
                    Value::Const(Constant(n)) => quote! {
                        declare_fixed_opaque(#name, #n)
                    },
                    _ => quote! {
                        Declaration::FixedOpaque { name: ident(#name), size: #size }
                    },
                },
            Declaration::VariableArray { name: Ident(name), elt_ty, max } => quote! {
                declare_variable_array(#name, #elt_ty, #max)
            },
            Declaration::VariableOpaque { name: Ident(name), max } => quote! {
                declare_variable_opaque(#name, #max),
            },
            Declaration::VariableString { name: Ident(name), max } => quote! {
                declare_variable_string(#name, #max),
            },
            Declaration::Optional { name: Ident(name), opt_ty } => quote! {
                declare_optional(#name, #opt_ty)
            },
            Declaration::Void => quote! {
                declare_void()
            },
        });
    }
}
