use xdr_repr::*;
use xdrs::*;

#[derive(XDR)]
pub struct Measurement {
    pub temperature: f32,
    pub ratio: f64,
}

check_format! { derive_float_struct
    Measurement::def()
    =>
        "struct Measurement {"
        "	float temperature;"
        "	double ratio;"
        "};"
}

#[test]
fn float_json_schema() {
    let type_def = Measurement::type_def();
    if let TypeDef::Struct(_, body) = type_def {
        let json = TypeSpec::Struct(body).get_type_json();
        assert!(
            json.contains("\"type\": \"float\""),
            "expected float type in JSON: {json}"
        );
        assert!(
            json.contains("\"type\": \"double\""),
            "expected double type in JSON: {json}"
        );
    } else {
        panic!("expected struct TypeDef");
    }
}
