use xdr_repr::*;

pub struct Boo {
    pub boonum: u32,
}

impl HasTypeDef for Boo {
    fn type_def() -> TypeDef {
        struct_def("Boo", vec![<u32 as Declare>::declare(ident("boonum"))])
    }
}

#[test]
fn test_result_type() {
    assert_eq!(<Result<(), ()>>::type_spec().to_string(), "ExpectedVoid");
    assert_eq!(<Result<Boo, ()>>::type_spec().to_string(), "ExpectedBoo");
}
