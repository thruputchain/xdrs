//! Representation of an XDR specification.
//!
//! This module provides types for representing the structure of [XDR][RFC-4506] specifications
//! as defined in RFC 4506.
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// A namespace containing an XDR specification.
///
/// Namespaces provide C++-like scoping for XDR definitions, allowing multiple
/// specifications with the same type names to coexist.
#[cfg(feature = "ns")]
#[cfg_attr(docsrs, doc(cfg(feature = "ns")))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// The name of the namespace.
    pub name: String,
    /// The XDR specification contained in this namespace.
    pub spec: Specification,
}

/// A complete XDR specification.
///
/// A specification contains a list of definitions (types and constants)
/// that together form a complete XDR specification document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specification {
    /// The list of definitions in this specification.
    pub definitions: Vec<Definition>,
}

/// A single definition in an XDR specification.
///
/// Definitions can be either type definitions or constant definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Definition {
    /// A type definition.
    Type(TypeDef),
    /// A constant definition.
    Const(ConstDef),
}

/// A type definition in XDR.
///
/// Type definitions can be:
/// - A simple typedef (wrapping a declaration)
/// - An enum type
/// - A struct type
/// - A union type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDef {
    /// A typedef wrapping a declaration.
    Def(Declaration),
    /// An enum type definition.
    Enum(Ident, EnumBody),
    /// A struct type definition.
    Struct(Ident, StructBody),
    /// A union type definition.
    Union(Ident, UnionBody),
}

/// A constant definition in XDR.
///
/// Constants associate a name with a constant value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstDef {
    /// The name of the constant.
    pub ident: Ident,
    /// The constant value.
    pub constant: Constant,
}

/// An identifier (name) in XDR.
///
/// Identifiers are used for type names, field names, constant names, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident(pub String);

/// The body of a struct type definition.
///
/// Contains the list of field declarations that make up the struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructBody(pub Vec<Declaration>);

/// The body of an enum type definition.
///
/// Contains pairs of identifier names and their associated values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumBody(pub Vec<(Ident, Value)>);

/// The body of a union type definition.
///
/// Unions are discriminated by a discriminant field and contain
/// case specifications for each possible discriminant value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionBody {
    /// The declaration of the discriminant field.
    pub discriminant_decl: Declaration,
    /// The case specifications. Must be non-empty.
    pub case_specs: Vec<CaseSpec>,
    /// An optional default case declaration.
    pub default_decl: Option<Declaration>,
}

/// A case specification in a union.
///
/// Specifies the discriminant values that trigger this case
/// and the declaration for the case's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseSpec {
    /// The discriminant values that trigger this case. Must be non-empty.
    pub cases: Vec<Value>,
    /// The declaration for this case's data.
    pub decl: Declaration,
}

/// A constant value in XDR.
///
/// Constants can be either signed or unsigned 32-bit integers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constant {
    /// An unsigned constant.
    Unsigned(u32),
    /// A signed constant.
    Signed(i32),
}

/// A value expression in XDR.
///
/// Values can be either constant literals or references to named constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A constant value.
    Const(Constant),
    /// A reference to a named constant.
    Ident(Ident),
}

/// A declaration in XDR.
///
/// Declarations specify a name and a type, representing fields in structs,
/// parameters in procedures, or variables in general.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declaration {
    /// A named declaration with a type specification.
    Named {
        /// The name of the declared item.
        name: Ident,
        /// The type specification.
        ty: TypeSpec,
    },
    /// A fixed-length opaque data declaration.
    FixedOpaque {
        /// The name of the opaque data.
        name: Ident,
        /// The fixed size of the opaque data.
        size: Value,
    },
    /// A variable-length opaque data declaration.
    VariableOpaque {
        /// The name of the opaque data.
        name: Ident,
        /// The maximum length of the opaque data.
        max: Value,
    },
    /// A variable-length string declaration.
    String { name: Ident, max: Value },
    /// A fixed-length array declaration.
    FixedArray {
        /// The name of the array.
        name: Ident,
        /// The type specification of the array elements.
        elt_ty: TypeSpec,
        /// The fixed length of the array.
        len: Value,
    },
    /// A variable-length array declaration.
    VariableArray {
        /// The name of the array.
        name: Ident,
        /// The type specification of the array elements.
        elt_ty: TypeSpec,
        /// The maximum length of the array.
        max: Value,
    },
    /// An optional data declaration.
    Optional {
        /// The name of the optional data.
        name: Ident,
        /// The type specification of the optional data.
        opt_ty: TypeSpec,
    },
    /// A void declaration (used for union cases with no data).
    Void,
}

impl Declaration {
    /// Returns the name of this declaration, or None for void declarations.
    pub fn name(&self) -> Option<&Ident> {
        match self {
            Declaration::Named { name, .. } => Some(name),
            Declaration::FixedOpaque { name, .. } => Some(name),
            Declaration::VariableOpaque { name, .. } => Some(name),
            Declaration::String { name, .. } => Some(name),
            Declaration::FixedArray { name, .. } => Some(name),
            Declaration::VariableArray { name, .. } => Some(name),
            Declaration::Optional { name, .. } => Some(name),
            Declaration::Void => None,
        }
    }

    /// Returns a JSON-parseable description of this declaration's type structure.
    ///
    /// This method generates a JSON string describing the type information of the
    /// declaration, excluding the name. The output can be parsed by standard JSON
    /// parsers for programmatic type introspection.
    ///
    /// # Format
    ///
    /// The output format varies by declaration type:
    /// - Named types: `{ "type": "<type_spec>" }`
    /// - Fixed opaque: `{ "type": "fixed opaque", "len": <size> }`
    /// - Variable opaque: `{ "type": "variable opaque", "max": <max> }`
    /// - String: `{ "type": "string", "max": <max> }`
    /// - Fixed array: `{ "type": "fixed array", "elt_ty": <element_type>, "len": <length> }`
    /// - Variable array: `{ "type": "variable array", "elt_ty": <element_type>, "max": <max> }`
    /// - Optional: `{ "type": "optional", "opt_ty": <wrapped_type> }`
    /// - Void: `{ "type": "void" }`
    ///
    /// # Limitations
    ///
    /// Custom type identifiers are inserted without escaping special characters.
    /// Identifiers containing quotes or other special characters may produce malformed JSON.
    ///
    /// # Panics
    ///
    /// Panics if the declaration contains embedded enum or union types.
    /// These types are not yet supported. Define them at the top level and reference
    /// them by name instead. Embedded struct types are supported.
    ///
    /// # Examples
    ///
    /// ```
    /// use xdr_repr::*;
    ///
    /// // Void declaration
    /// let void_decl = Declaration::Void;
    /// assert_eq!(void_decl.get_type_json(), "{ \"type\": \"void\" }");
    ///
    /// // Named type with primitive
    /// let int_decl = declare_type("value", TypeSpec::Int);
    /// assert_eq!(int_decl.get_type_json(), "{ \"type\": \"int\" }");
    ///
    /// // Fixed opaque array
    /// let opaque_decl = declare_fixed_opaque("data", 10);
    /// assert_eq!(opaque_decl.get_type_json(), "{ \"type\": \"fixed opaque\", \"len\": 10 }");
    /// ```
    pub fn get_type_json(&self) -> String {
        use alloc::format;
        match self {
            Declaration::Named { name: _, ty } => {
                let type_json = ty.get_type_json();
                // Check if this is a complex type (starts with '{') or a simple type name
                if type_json.starts_with('{') {
                    // Complex type (struct, etc.) - use as-is for the "type" field value
                    format!("{{ \"type\": {} }}", type_json)
                } else {
                    // Simple type name - quote it
                    format!("{{ \"type\": \"{}\" }}", type_json)
                }
            }
            Declaration::FixedOpaque { name: _, size } => {
                format!("{{ \"type\": \"fixed opaque\", \"len\": {} }}", size)
            }
            Declaration::VariableOpaque { name: _, max } => {
                format!("{{ \"type\": \"variable opaque\", \"max\": {} }}", max)
            }
            Declaration::String { name: _, max } => {
                format!("{{ \"type\": \"string\", \"max\": {} }}", max)
            }
            Declaration::FixedArray {
                name: _,
                elt_ty,
                len,
            } => {
                format!(
                    "{{ \"type\": \"fixed array\", \"elt_ty\": \"{}\", \"len\": {} }}",
                    elt_ty.get_type_json(),
                    len
                )
            }
            Declaration::VariableArray {
                name: _,
                elt_ty,
                max,
            } => {
                format!(
                    "{{ \"type\": \"variable array\", \"elt_ty\": \"{}\", \"max\": {} }}",
                    elt_ty.get_type_json(),
                    max
                )
            }
            Declaration::Optional { name: _, opt_ty } => {
                format!(
                    "{{ \"type\": \"optional\", \"opt_ty\": \"{}\" }}",
                    opt_ty.get_type_json()
                )
            }
            Declaration::Void => {
                "{ \"type\": \"void\" }".to_string()
            }
        }
    }
}

/// A type specification in XDR.
///
/// Type specifications describe the type of a value, field, or parameter.
/// They can be primitive types, named types, or inline type definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeSpec {
    /// Signed 32-bit integer (`int`).
    Int,
    /// Unsigned 32-bit integer (`unsigned int`).
    Unsigned,
    /// Signed 64-bit integer (`hyper`).
    Hyper,
    /// Unsigned 64-bit integer (`unsigned hyper`).
    UnsignedHyper,
    /// 32-bit floating point (`float`).
    Float,
    /// 64-bit floating point (`double`).
    Double,
    /// 128-bit floating point (`quadruple`).
    Quadruple,
    /// Boolean type (`bool`).
    Bool,
    /// Inline enum type definition.
    Enum(EnumBody),
    /// Inline struct type definition.
    Struct(StructBody),
    /// Inline union type definition.
    Union(Box<UnionBody>),
    /// Reference to a named type.
    Ident(Ident),
}

impl TypeSpec {
    /// Returns the type name as a string for use in JSON type descriptions.
    ///
    /// This method returns a simple string representing the type, which can be
    /// embedded in the JSON output generated by [`Declaration::get_type_json`].
    ///
    /// # Supported Types
    ///
    /// - Primitive types: Returns the type name as a string (`"int"`, `"unsigned"`, etc.)
    /// - Named types: Returns the identifier name
    /// - Struct types: Returns a JSON object with type "struct" and a "fields" array
    ///
    /// # Limitations
    ///
    /// Custom type identifiers are not escaped, which may result in invalid JSON
    /// if the identifier contains quotes or other special characters.
    ///
    /// # Panics
    ///
    /// Panics for embedded enum and union types. These types are not yet supported.
    /// Define them at the top level and reference them by name instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use xdr_repr::*;
    ///
    /// // Primitive types
    /// assert_eq!(TypeSpec::Int.get_type_json(), "int");
    /// assert_eq!(TypeSpec::Unsigned.get_type_json(), "unsigned");
    /// assert_eq!(TypeSpec::Bool.get_type_json(), "bool");
    ///
    /// // Named type
    /// let custom_type = TypeSpec::Ident(ident("CustomType"));
    /// assert_eq!(custom_type.get_type_json(), "CustomType");
    ///
    /// // Embedded struct type
    /// let struct_type = TypeSpec::Struct(StructBody(vec![
    ///     declare_type("id", TypeSpec::Unsigned),
    ///     declare_type("name", TypeSpec::Ident(ident("String"))),
    /// ]));
    /// let json = struct_type.get_type_json();
    /// // Returns: { "type": "struct", "fields": [{ "name": "id", "field_type": { "type": "unsigned" } }, ...] }
    /// ```
    pub fn get_type_json(&self) -> alloc::string::String {
        use alloc::format;
        use alloc::string::String;

        match self {
            TypeSpec::Int => "int".to_string(),
            TypeSpec::Unsigned => "unsigned".to_string(),
            TypeSpec::Hyper => "hyper".to_string(),
            TypeSpec::UnsignedHyper => "unsigned hyper".to_string(),
            TypeSpec::Float => "float".to_string(),
            TypeSpec::Double => "double".to_string(),
            TypeSpec::Quadruple => "quadruple".to_string(),
            TypeSpec::Bool => "bool".to_string(),
            TypeSpec::Enum(_body) => panic!("Embedded enum type not yet supported"),
            TypeSpec::Struct(body) => {
                let mut result = String::from("{ \"type\": \"struct\", \"fields\": [");
                let mut first = true;
                for decl in &body.0 {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;

                    if let Some(name) = decl.name() {
                        result.push_str(&format!(
                            "{{ \"name\": \"{}\", \"field_type\": {} }}",
                            name.0,
                            decl.get_type_json()
                        ));
                    } else {
                        // Void field - shouldn't normally happen in a struct, but handle it
                        result.push_str("{ \"name\": null, \"field_type\": { \"type\": \"void\" } }");
                    }
                }
                result.push_str("] }");
                result
            }
            TypeSpec::Union(_body) => panic!("Embedded union type not yet supported"),
            TypeSpec::Ident(ident) => ident.0.clone(),
        }
    }
}
