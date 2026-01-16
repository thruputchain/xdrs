use xdr_repr::*;

#[test]
fn void_declaration_json() {
    let void_decl = Declaration::Void;
    let json = void_decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"void\" }");
}

#[test]
fn named_declaration_json() {
    let named_decl = declare_type("value", TypeSpec::Int);
    let json = named_decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"int\" }");
}

#[test]
fn fixed_opaque_declaration_json() {
    let fixed_opaque = declare_fixed_opaque("data", 10);
    let json = fixed_opaque.get_type_json();
    assert_eq!(json, "{ \"type\": \"fixed opaque\", \"len\": 10 }");
}

#[test]
fn variable_opaque_declaration_json() {
    let var_opaque = Declaration::VariableOpaque {
        name: ident("data"),
        max: unsigned(100),
    };
    let json = var_opaque.get_type_json();
    assert_eq!(json, "{ \"type\": \"variable opaque\", \"max\": 100 }");
}

#[test]
fn string_declaration_json() {
    let string_decl = Declaration::String {
        name: ident("name"),
        max: unsigned(50),
    };
    let json = string_decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"string\", \"max\": 50 }");
}

#[test]
fn fixed_array_declaration_json() {
    let fixed_array = Declaration::FixedArray {
        name: ident("items"),
        elt_ty: TypeSpec::Unsigned,
        len: unsigned(5),
    };
    let json = fixed_array.get_type_json();
    assert_eq!(json, "{ \"type\": \"fixed array\", \"elt_ty\": unsigned, \"len\": 5 }");
}

#[test]
fn optional_declaration_json() {
    let optional_decl = Declaration::Optional {
        name: ident("value"),
        opt_ty: TypeSpec::Bool,
    };
    let json = optional_decl.get_type_json();
    assert_eq!(json, "{ \"type\": \"optional\", \"opt_ty\": bool }");
}
