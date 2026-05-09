use xdr_repr::*;

#[test]
fn ref_primitive_has_type_spec() {
    assert_eq!(<&i32>::type_spec(), i32::type_spec());
    assert_eq!(<&mut u32>::type_spec(), u32::type_spec());
    assert_eq!(<&bool>::type_spec(), bool::type_spec());
}

#[test]
fn ref_bytesm_has_type_spec() {
    assert_eq!(<&xdr_common::BytesM<256>>::type_spec(), xdr_common::BytesM::<256>::type_spec());
    assert_eq!(<&mut xdr_common::BytesM<256>>::type_spec(), xdr_common::BytesM::<256>::type_spec());
}

#[test]
fn ref_stringm_has_type_spec() {
    assert_eq!(<&xdr_common::StringM<128>>::type_spec(), xdr_common::StringM::<128>::type_spec());
    assert_eq!(<&mut xdr_common::StringM<128>>::type_spec(), xdr_common::StringM::<128>::type_spec());
}

#[test]
fn ref_vecm_has_type_spec() {
    assert_eq!(<&xdr_common::VecM<u32, 10>>::type_spec(), xdr_common::VecM::<u32, 10>::type_spec());
    assert_eq!(<&mut xdr_common::VecM<u32, 10>>::type_spec(), xdr_common::VecM::<u32, 10>::type_spec());
    assert_eq!(<&xdr_common::VecM<u8, 64>>::type_spec(), xdr_common::VecM::<u8, 64>::type_spec());
    assert_eq!(<&mut xdr_common::VecM<u8, 64>>::type_spec(), xdr_common::VecM::<u8, 64>::type_spec());
}

#[test]
fn ref_vec_has_type_spec() {
    assert_eq!(<&Vec<u8>>::type_spec(), Vec::<u8>::type_spec());
    assert_eq!(<&mut Vec<u8>>::type_spec(), Vec::<u8>::type_spec());
    assert_eq!(<&Vec<u32>>::type_spec(), Vec::<u32>::type_spec());
    assert_eq!(<&mut Vec<u32>>::type_spec(), Vec::<u32>::type_spec());
}

#[test]
fn ref_string_has_type_spec() {
    assert_eq!(<&String>::type_spec(), String::type_spec());
    assert_eq!(<&mut String>::type_spec(), String::type_spec());
}

#[test]
fn ref_slice_has_type_spec() {
    assert_eq!(<&mut [u8]>::type_spec(), <&[u8]>::type_spec());
    assert_eq!(<&mut str>::type_spec(), <&str>::type_spec());
    assert_eq!(<&[u32]>::type_spec(), <&[u32]>::type_spec());
    assert_eq!(<&mut [u32]>::type_spec(), <&[u32]>::type_spec());
}

#[test]
fn ref_fixed_array_has_type_spec() {
    assert_eq!(<&[u32; 5]>::type_spec(), <[u32; 5]>::type_spec());
    assert_eq!(<&mut [u32; 5]>::type_spec(), <[u32; 5]>::type_spec());
}

#[test]
fn option_of_ref_compiles() {
    let _ = <Option<&xdr_common::BytesM<256>>>::type_spec();
    let _ = <Option<&xdr_common::StringM<128>>>::type_spec();
    let _ = <Option<&xdr_common::VecM<u32, 10>>>::type_spec();
    let _ = <Option<&Vec<u8>>>::type_spec();
    let _ = <Option<&i32>>::type_spec();
    let _ = <Option<&mut u32>>::type_spec();
}

#[test]
fn ref_result_has_type_spec() {
    assert_eq!(
        <&Result<u32, i32>>::type_spec(),
        Result::<u32, i32>::type_spec()
    );
    assert_eq!(
        <&mut Result<u32, i32>>::type_spec(),
        Result::<u32, i32>::type_spec()
    );
}
