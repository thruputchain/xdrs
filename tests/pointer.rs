mod common;
use common::*;

#[cfg(feature = "alloc")]
#[test]
fn recursive_struct() {
    use xdrs::*;
    
    #[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
    #[repr(C)]
    struct Recursive {
        pub elem: BytesM,
        pub next: Option<Box<Recursive>>,
        pub nextvec: VecM<Recursive>,
    }

    let val = Recursive {
        elem: BytesM("hello".as_bytes().into()),
        next: None,
        nextvec: VecM::default(),
    };

    CXS::check_codec(&val, &hex!("0568656c6c6f0000"));
    TXS::check_codec(&val, &hex!("0000000568656c6c6f0000000000000000000000"));
}