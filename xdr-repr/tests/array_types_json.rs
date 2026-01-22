use xdr_repr::*;
use xdrs::*;

// Tests for JSON spec generation with array types, particularly [u8; N]

#[test]
fn byte_array_fixed_opaque_json() {
    // [u8; N] should map to fixed opaque, not fixed array
    let decl = <[u8; 10]>::declare(ident("data"));
    let json = decl.get_type_json();

    assert_eq!(
        json,
        r#"{ "type": "fixed opaque", "len": 10 }"#
    );
}

#[test]
fn byte_array_various_sizes() {
    // Test different sizes of byte arrays
    let sizes = [1, 16, 32, 64, 128, 256, 512, 1024];

    for size in sizes {
        let json = match size {
            1 => <[u8; 1]>::declare(ident("data")).get_type_json(),
            16 => <[u8; 16]>::declare(ident("data")).get_type_json(),
            32 => <[u8; 32]>::declare(ident("data")).get_type_json(),
            64 => <[u8; 64]>::declare(ident("data")).get_type_json(),
            128 => <[u8; 128]>::declare(ident("data")).get_type_json(),
            256 => <[u8; 256]>::declare(ident("data")).get_type_json(),
            512 => <[u8; 512]>::declare(ident("data")).get_type_json(),
            1024 => <[u8; 1024]>::declare(ident("data")).get_type_json(),
            _ => unreachable!(),
        };

        assert_eq!(
            json,
            format!(r#"{{ "type": "fixed opaque", "len": {} }}"#, size)
        );
    }
}

#[test]
fn non_byte_array_fixed_array_json() {
    // [u32; N] should map to fixed array with element type
    let decl = <[u32; 10]>::declare(ident("values"));
    let json = decl.get_type_json();

    assert_eq!(
        json,
        r#"{ "type": "fixed array", "elt_ty": "unsigned", "len": 10 }"#
    );
}

#[test]
fn various_primitive_arrays() {
    // Test arrays of different primitive types
    assert_eq!(
        <[i32; 5]>::declare(ident("ints")).get_type_json(),
        r#"{ "type": "fixed array", "elt_ty": "int", "len": 5 }"#
    );

    assert_eq!(
        <[u32; 5]>::declare(ident("uints")).get_type_json(),
        r#"{ "type": "fixed array", "elt_ty": "unsigned", "len": 5 }"#
    );

    assert_eq!(
        <[i64; 5]>::declare(ident("hypers")).get_type_json(),
        r#"{ "type": "fixed array", "elt_ty": "hyper", "len": 5 }"#
    );

    assert_eq!(
        <[u64; 5]>::declare(ident("uhypers")).get_type_json(),
        r#"{ "type": "fixed array", "elt_ty": "unsigned hyper", "len": 5 }"#
    );

    assert_eq!(
        <[bool; 5]>::declare(ident("flags")).get_type_json(),
        r#"{ "type": "fixed array", "elt_ty": "bool", "len": 5 }"#
    );
}

#[test]
fn byte_array_in_derived_struct() {
    // Test [u8; N] in a derived struct
    #[derive(XDR)]
    struct Message {
        _id: u32,
        _data: [u8; 16],
        _signature: [u8; 64],
    }

    let type_def = Message::type_def();

    if let TypeDef::Struct(name, body) = type_def {
        assert_eq!(name.0, "Message");
        assert_eq!(body.0.len(), 3);

        // First field should be unsigned
        assert_eq!(
            body.0[0].get_type_json(),
            r#"{ "type": "unsigned" }"#
        );

        // Second field should be fixed opaque (not fixed array!)
        assert_eq!(
            body.0[1].get_type_json(),
            r#"{ "type": "fixed opaque", "len": 16 }"#
        );

        // Third field should also be fixed opaque
        assert_eq!(
            body.0[2].get_type_json(),
            r#"{ "type": "fixed opaque", "len": 64 }"#
        );

        // Test full struct JSON
        let struct_json = TypeSpec::Struct(body).get_type_json();
        assert_eq!(
            struct_json,
            r#"{
  "type": "struct",
  "fields": [
    {
      "name": "_id",
      "field_type": { "type": "unsigned" }
    },
    {
      "name": "_data",
      "field_type": { "type": "fixed opaque", "len": 16 }
    },
    {
      "name": "_signature",
      "field_type": { "type": "fixed opaque", "len": 64 }
    }
  ]
}"#
        );
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn mixed_arrays_in_struct() {
    // Test struct with both byte arrays and other arrays
    #[derive(XDR)]
    struct MixedArrays {
        _hash: [u8; 32],
        _counts: [u32; 10],
        _nonce: [u8; 8],
        _flags: [bool; 5],
    }

    let type_def = MixedArrays::type_def();

    if let TypeDef::Struct(_, body) = type_def {
        assert_eq!(body.0.len(), 4);

        // Byte arrays should be fixed opaque
        assert_eq!(
            body.0[0].get_type_json(),
            r#"{ "type": "fixed opaque", "len": 32 }"#
        );

        // u32 array should be fixed array
        assert_eq!(
            body.0[1].get_type_json(),
            r#"{ "type": "fixed array", "elt_ty": "unsigned", "len": 10 }"#
        );

        // Another byte array should be fixed opaque
        assert_eq!(
            body.0[2].get_type_json(),
            r#"{ "type": "fixed opaque", "len": 8 }"#
        );

        // bool array should be fixed array
        assert_eq!(
            body.0[3].get_type_json(),
            r#"{ "type": "fixed array", "elt_ty": "bool", "len": 5 }"#
        );
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn byte_array_xdr_spec() {
    // Verify XDR spec output for byte arrays
    #[derive(XDR)]
    struct Data {
        _key: [u8; 32],
    }

    let def = Data::def();
    let def_str = format!("{}", def);

    assert_eq!(
        def_str,
        r#"struct Data {
	opaque _key[32];
};
"#
    );
}

#[test]
fn mixed_arrays_xdr_spec() {
    // Verify XDR spec distinguishes opaque from regular arrays
    #[derive(XDR)]
    struct MixedData {
        _bytes: [u8; 16],
        _numbers: [u32; 16],
    }

    let def = MixedData::def();
    let def_str = format!("{}", def);

    assert_eq!(
        def_str,
        r#"struct MixedData {
	opaque _bytes[16];
	unsigned int _numbers[16];
};
"#
    );
}

#[test]
fn nested_byte_arrays() {
    // Test [[u8; 3]; 5] - nested byte arrays
    #[derive(XDR)]
    struct NestedBytes {
        _matrix: [[u8; 3]; 5],
    }

    let type_def = NestedBytes::type_def();

    if let TypeDef::Struct(_, body) = type_def {
        assert_eq!(body.0.len(), 1);

        let json = body.0[0].get_type_json();

        // The outer array should be a fixed array
        assert!(json.contains(r#""type": "fixed array""#));
        assert!(json.contains(r#""len": 5"#));

        // The element type should be an embedded struct containing a fixed opaque
        assert!(json.contains(r#""fixed opaque""#));
        assert!(json.contains(r#""len": 3"#));

        // Full expected structure:
        // fixed array of 5 elements, where each element is a struct containing
        // a fixed opaque array of 3 bytes
        // The element type is embedded but the JSON should be parseable
        assert!(json.contains(r#""type": "fixed array""#));
        assert!(json.contains(r#""len": 5"#));
        assert!(json.contains(r#""type": "struct""#));
        assert!(json.contains(r#""name": "elements""#));
        assert!(json.contains(r#""type": "fixed opaque""#));
        assert!(json.contains(r#""len": 3"#));
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn nested_typed_arrays() {
    // Test [[u32; 3]; 4] - nested typed arrays
    #[derive(XDR)]
    struct Matrix {
        _data: [[u32; 3]; 4],
    }

    let type_def = Matrix::type_def();

    if let TypeDef::Struct(_, body) = type_def {
        assert_eq!(body.0.len(), 1);

        let json = body.0[0].get_type_json();

        // Should be a fixed array of 4 elements
        assert!(json.contains(r#""len": 4"#));

        // Each element is a struct containing a fixed array of 3 u32s
        assert!(json.contains(r#""fixed array""#));
        assert!(json.contains(r#""len": 3"#));
        assert!(json.contains(r#""elt_ty": "unsigned""#));
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn triple_nested_arrays() {
    // Test [[[u32; 2]; 3]; 4] - triple nested arrays
    #[derive(XDR)]
    struct Tensor {
        _data: [[[u32; 2]; 3]; 4],
    }

    let type_def = Tensor::type_def();

    if let TypeDef::Struct(_, body) = type_def {
        assert_eq!(body.0.len(), 1);

        let json = body.0[0].get_type_json();

        // Should have all three levels
        assert!(json.contains(r#""len": 4"#));  // outer
        assert!(json.contains(r#""len": 3"#));  // middle
        assert!(json.contains(r#""len": 2"#));  // inner
    } else {
        panic!("Expected TypeDef::Struct");
    }
}

#[test]
fn nested_byte_array_xdr_spec() {
    // Verify XDR spec output for nested byte arrays
    #[derive(XDR)]
    struct Data {
        _keys: [[u8; 32]; 10],
    }

    let def = Data::def();
    let def_str = format!("{}", def);

    // Should show a struct with embedded struct for the array element type
    assert!(def_str.contains("struct Data"));
    assert!(def_str.contains("_keys"));
}

#[test]
fn byte_array_comparison_with_other_types() {
    // Direct comparison: byte array vs other types

    // Byte array should be "fixed opaque"
    let byte_array = <[u8; 10]>::declare(ident("bytes"));
    assert!(byte_array.get_type_json().contains("fixed opaque"));
    assert!(!byte_array.get_type_json().contains("fixed array"));

    // u32 array should be "fixed array"
    let u32_array = <[u32; 10]>::declare(ident("numbers"));
    assert!(u32_array.get_type_json().contains("fixed array"));
    assert!(!u32_array.get_type_json().contains("fixed opaque"));

    // They should have different JSON representations
    assert_ne!(byte_array.get_type_json(), u32_array.get_type_json());
}
