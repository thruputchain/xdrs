mod common;
use xdrs::{ReadResult, ReadXDR, Reader, State, WriteXDR, read_xdr};

#[test]
fn stateful() {
    #[derive(Debug, PartialEq, Eq, WriteXDR)]
    struct Foo {
        m: i32,
        n: u32,
    }

    impl<'r> ReadXDR<'r, u32> for Foo {
        fn read_xdr<R>(r: &mut R) -> ReadResult<Self>
        where
            R: Reader<'r> + State<u32>
        {
            let m: i32 = read_xdr(r)?;
            let _n: u32 = read_xdr(r)?;
            let n: u32 = <R as State<u32>>::get_state(r).clone();

            Ok(Foo { m, n })
        }
    }

    // Given: a value and its expected encoding
    let val: Foo = Foo {
        m: -3,
        n: 7,
    };

    let bytes = &hex!("00000007");
}
