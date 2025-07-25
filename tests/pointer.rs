mod common;
use common::*;


#[cfg(feature = "alloc")]
#[test]
fn vecm_empty() {
    use xdrs::*;

    let val: VecM<u32> = VecM::default();

    CXS::check_codec(&val, &hex!("00"));
    TXS::check_codec(&val, &hex!("00000000"));
}

#[cfg(feature = "alloc")]
#[test]
fn vecm_full() {
    use xdrs::*;

    let val: VecM<u32> = VecM(vec![1]);

    CXS::check_codec(&val, &hex!("0101"));
    TXS::check_codec(&val, &hex!("0000000100000001"));
}

#[cfg(feature = "alloc")]
#[test]
fn vec_in_struct() {
    use xdrs::*;

    #[derive(Debug, PartialEq, Eq, xdrs::ReadXDR, xdrs::WriteXDR)]
    #[repr(C)]
    struct Two {
        pub a: Vec<u8>,
        pub b: bool,
    }

    let val = Two {
        a: Vec::default(),
        b: true,
    };

    CXS::check_decode(&val, &hex!("0001"));
    TXS::check_codec(&val, &hex!("0000000000000001"));
}

#[test]
fn slice_in_struct() {
    let arr: [u8; 0] = [];
    let buf = &arr[..];

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(C)]
    struct Two<'a> {
        pub a: &'a [u8],
        pub b: bool,
    }

    let val = Two {
        a: buf,
        b: true,
    };

    CXS::check_codec(&val, &hex!("0001"));
    TXS::check_codec(&val, &hex!("0000000000000001"));
}

#[cfg(feature = "alloc")]
#[test]
fn vec_in_tuple() {
    use xdrs::*;

    let val = (Vec::<u32>::default(), true);

    CXS::check_encode(&val, &hex!("0001"));
    CXS::check_decode(&val, &hex!("0001"));
}

#[cfg(feature = "alloc")]
#[test]
fn recursive_struct() {
    use xdrs::*;

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(C)]
    struct Recursive {
        pub elem: BytesM,
        pub nextvec: VecM<Recursive>,
    }

    let val = Recursive {
        elem: BytesM("hello".as_bytes().into()),
        nextvec: VecM::default(),
    };

    CXS::check_codec(&val, &hex!("0568656c6c6f00"));
    TXS::check_codec(&val, &hex!("0000000568656c6c6f00000000000000"));
}

#[cfg(feature = "alloc")]
#[test]
fn recursive_struct_box() {
    use xdrs::*;

    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(C)]
    struct Recursive {
        pub elem: BytesM,
        pub next: Option<Box<Recursive>>,
    }

    let val = Recursive {
        elem: BytesM("hello".as_bytes().into()),
        next: None,
    };

    CXS::check_codec(&val, &hex!("0568656c6c6f00"));
    TXS::check_codec(&val, &hex!("0000000568656c6c6f00000000000000"));
}

#[test]
fn option_test() {
    let x : Option<i32> = None;
    let y : Option<i32> = Some(1);

    CXS::check_codec(&x, &hex!("00"));
    TXS::check_codec(&x, &hex!("00000000"));

    CXS::check_codec(&y, &hex!("0101"));
    TXS::check_codec(&y, &hex!("0000000100000001"));
}

#[cfg(feature = "alloc")]
#[test]
fn option_box_test() {
    let x : Option<Box<i32>> = None;
    let y : Option<Box<i32>> = Some(Box::new(1));

    CXS::check_codec(&x, &hex!("00"));
    TXS::check_codec(&x, &hex!("00000000"));

    CXS::check_codec(&y, &hex!("0101"));
    TXS::check_codec(&y, &hex!("0000000100000001"));
}
