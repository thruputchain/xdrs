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

/// Internal representation of type JSON - either a simple name or a complex object.
#[derive(Debug)]
enum TypeJsonRepr {
    /// Simple type name like "int" or "CustomType"
    Name(alloc::string::String),
    /// Complex JSON object like { "type": "struct", ... }
    Object(alloc::string::String),
}

impl TypeJsonRepr {
    fn into_string(self) -> alloc::string::String {
        match self {
            TypeJsonRepr::Name(s) | TypeJsonRepr::Object(s) => s,
        }
    }
}

/// Validates that a string is a valid XDR identifier.
///
/// According to RFC 4506 section 6.1, XDR identifiers must:
/// - Start with a letter (a-z, A-Z) or underscore (_)
/// - Contain only letters, digits (0-9), and underscores (_)
/// - Be pure ASCII (no Unicode characters)
///
/// Note: Leading underscores are allowed as an extension for Rust compatibility,
/// though strict RFC 4506 requires starting with a letter.
///
/// # Panics
///
/// Panics if the string is not a valid identifier, with a descriptive error message.
fn validate_xdr_identifier(s: &str) {
    if s.is_empty() {
        panic!("XDR identifier cannot be empty");
    }

    let mut chars = s.chars();

    // First character must be a letter or underscore
    if let Some(first) = chars.next() {
        if !first.is_ascii_alphabetic() && first != '_' {
            panic!(
                "Invalid XDR identifier '{}': must start with a letter (a-z, A-Z) or underscore (_), found '{}'",
                s, first
            );
        }
    }

    // Remaining characters must be letters, digits, or underscores
    for (idx, ch) in chars.enumerate() {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            panic!(
                "Invalid XDR identifier '{}': character at position {} ('{}') is not allowed. \
                 XDR identifiers can only contain ASCII letters, digits, and underscores.",
                s, idx + 1, ch
            );
        }
    }
}

/// Returns the identifier string after validating it's a valid XDR identifier.
/// Panics if the identifier is invalid.
fn get_validated_identifier(s: &str) -> &str {
    validate_xdr_identifier(s);
    s
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
    /// - Fixed array: `{ "type": "fixed array", "element_type": <element_type>, "len": <length> }`
    /// - Variable array: `{ "type": "variable array", "element_type": <element_type>, "max": <max> }`
    /// - Optional: `{ "type": "optional", "element_type": <wrapped_type> }`
    /// - Void: `{ "type": "void" }`
    ///
    /// # Panics
    ///
    /// **Panics if the declaration contains embedded enum or union types.**
    /// These types are not yet supported. Define them at the top level and reference
    /// them by name instead. Embedded struct types are fully supported.
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
                match ty.get_type_json_repr() {
                    TypeJsonRepr::Name(type_name) => {
                        // Simple type name - quote it (already validated in get_type_json_repr)
                        format!("{{ \"type\": \"{}\" }}", type_name)
                    }
                    TypeJsonRepr::Object(type_obj) => {
                        // Complex type (struct, etc.) - indent and embed it
                        let indented = type_obj.replace('\n', "\n  ");
                        format!("{}", indented)
                    }
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
                match elt_ty.get_type_json_repr() {
                    TypeJsonRepr::Name(type_name) => {
                        // Simple type name - quote it (already validated in get_type_json_repr)
                        format!(
                            "{{ \"type\": \"fixed array\", \"element_type\": {{ \"type\": \"{}\" }}, \"len\": {} }}",
                            type_name, len
                        )
                    }
                    TypeJsonRepr::Object(type_obj) => {
                        // Complex type (struct, etc.) - indent and embed it
                        let indented = type_obj.replace('\n', "\n  ");
                        format!(
                            "{{\n  \"type\": \"fixed array\",\n  \"element_type\": {},\n  \"len\": {}\n}}",
                            indented, len
                        )
                    }
                }
            }
            Declaration::VariableArray {
                name: _,
                elt_ty,
                max,
            } => {
                match elt_ty.get_type_json_repr() {
                    TypeJsonRepr::Name(type_name) => {
                        // Simple type name - quote it (already validated in get_type_json_repr)
                        format!(
                            "{{ \"type\": \"variable array\", \"element_type\": {{ \"type\": \"{}\" }}, \"max\": {} }}",
                            type_name, max
                        )
                    }
                    TypeJsonRepr::Object(type_obj) => {
                        // Complex type (struct, etc.) - indent and embed it
                        let indented = type_obj.replace('\n', "\n  ");
                        format!(
                            "{{\n  \"type\": \"variable array\",\n  \"element_type\": {},\n  \"max\": {}\n}}",
                            indented, max
                        )
                    }
                }
            }
            Declaration::Optional { name: _, opt_ty } => {
                match opt_ty.get_type_json_repr() {
                    TypeJsonRepr::Name(type_name) => {
                        // Simple type name - quote it (already validated in get_type_json_repr)
                        format!(
                            "{{ \"type\": \"optional\", \"element_type\": {{ \"type\": \"{}\" }} }}",
                            type_name
                        )
                    }
                    TypeJsonRepr::Object(type_obj) => {
                        // Complex type (struct, etc.) - indent and embed it
                        let indented = type_obj.replace('\n', "\n  ");
                        format!(
                            "{{\n  \"type\": \"optional\",\n  \"element_type\": {}\n}}",
                            indented
                        )
                    }
                }
            }
            Declaration::Void => "{ \"type\": \"void\" }".to_string(),
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
    /// - Named types: Returns the identifier name (validated as XDR identifier)
    /// - Struct types: Returns a JSON object with type "struct" and a "fields" array
    ///
    /// # Panics
    ///
    /// **Panics for embedded enum and union types.** These types are not yet supported.
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
        self.get_type_json_repr().into_string()
    }

    /// Internal method that returns the type representation with proper classification.
    fn get_type_json_repr(&self) -> TypeJsonRepr {
        use alloc::format;
        use alloc::string::String;

        match self {
            TypeSpec::Int => TypeJsonRepr::Name("int".to_string()),
            TypeSpec::Unsigned => TypeJsonRepr::Name("unsigned".to_string()),
            TypeSpec::Hyper => TypeJsonRepr::Name("hyper".to_string()),
            TypeSpec::UnsignedHyper => TypeJsonRepr::Name("unsigned hyper".to_string()),
            TypeSpec::Float => TypeJsonRepr::Name("float".to_string()),
            TypeSpec::Double => TypeJsonRepr::Name("double".to_string()),
            TypeSpec::Quadruple => TypeJsonRepr::Name("quadruple".to_string()),
            TypeSpec::Bool => TypeJsonRepr::Name("bool".to_string()),
            TypeSpec::Enum(_body) => panic!("Embedded enum type not yet supported"),
            TypeSpec::Struct(body) => {
                if body.0.is_empty() {
                    // Empty struct - keep it compact
                    return TypeJsonRepr::Object(
                        "{\n  \"type\": \"struct\",\n  \"fields\": []\n}".to_string(),
                    );
                }

                let mut result = String::from("{\n  \"type\": \"struct\",\n  \"fields\": [\n");
                let mut first = true;
                for decl in &body.0 {
                    if !first {
                        result.push_str(",\n");
                    }
                    first = false;

                    result.push_str("    ");
                    if let Some(name) = decl.name() {
                        let field_json = decl.get_type_json();
                        let validated_name = get_validated_identifier(&name.0);
                        result.push_str(&format!(
                            "{{\n      \"name\": \"{}\",\n      \"element_type\": {}\n    }}",
                            validated_name,
                            field_json.replace('\n', "\n      ")
                        ));
                    } else {
                        // Void field - shouldn't normally happen in a struct, but handle it
                        result.push_str("{\n      \"name\": null,\n      \"element_type\": { \"type\": \"void\" }\n    }");
                    }
                }
                result.push_str("\n  ]\n}");
                TypeJsonRepr::Object(result)
            }
            TypeSpec::Union(_body) => panic!("Embedded union type not yet supported"),
            TypeSpec::Ident(ident) => TypeJsonRepr::Name(get_validated_identifier(&ident.0).to_string()),
        }
    }
}
