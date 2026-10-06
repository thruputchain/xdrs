mod common;
use common::*;

use std::sync::Arc;
use xdrs::*;

#[derive(Debug, Clone, PartialEq, Eq, ReadXDR, WriteXDR)]
struct Tx {
    seq: u64,
    memo: StringM<16>,
}

type Batch = VecM<Tx, 4>;

#[derive(Debug, Clone, PartialEq, Eq, WriteXDR)]
struct Wrap<T: WriteXDR>(T);

fn txs() -> [Tx; 2] {
    [
        Tx {
            seq: 1,
            memo: "one".try_into().unwrap(),
        },
        Tx {
            seq: 2,
            memo: "two".try_into().unwrap(),
        },
    ]
}

// The shape of a generated RPC client method: the argument stands in for
// the wire type and is encoded as one element of the argument tuple.
fn encode_arg<X: ?Sized, F: Format>(arg: &(impl EncodesAs<X> + ?Sized)) -> Vec<u8> {
    F::encode(&(arg, 7u32)).unwrap()
}

// Encodes each stand-in both bare and through `encode_arg`, in both
// formats, and checks the bytes against those of the owned wire value.
macro_rules! check_stands_in {
    ($x:ty, $owned:expr, $( $stand_in:expr ),+ $(,)?) => {
        check_stands_in!(@format CXS, $x, $owned, $($stand_in),+);
        check_stands_in!(@format TXS, $x, $owned, $($stand_in),+);
    };
    (@format $f:ident, $x:ty, $owned:expr, $( $stand_in:expr ),+) => {{
        let owned: $x = $owned;
        let expected = $f::encode(&owned).unwrap();
        let expected_arg = encode_arg::<$x, $f>(&owned);
        $(
            $f::check_encode($stand_in, &expected);
            assert_eq!(encode_arg::<$x, $f>($stand_in), expected_arg);
        )+
    }};
}

#[test]
fn slices_of_pointers_stand_in_for_vecm() {
    let owned = txs();
    let arcs: Vec<Arc<Tx>> = owned.iter().cloned().map(Arc::new).collect();
    let refs: Vec<&Tx> = owned.iter().collect();
    let arc_refs: Vec<&Arc<Tx>> = arcs.iter().collect();
    let boxes: Vec<Box<Tx>> = owned.iter().cloned().map(Box::new).collect();
    let shared_slice: Arc<[Tx]> = owned.to_vec().into();

    check_stands_in!(
        Batch,
        Batch::try_from(owned.to_vec()).unwrap(),
        &owned[..],
        &arcs[..],
        &refs[..],
        &arc_refs[..],
        &boxes[..],
        &shared_slice,
    );
}

#[test]
fn pointers_stand_in_for_their_pointee() {
    let tx = txs()[0].clone();
    let arc = Arc::new(tx.clone());
    let boxed = Box::new(tx.clone());

    check_stands_in!(
        Tx,
        tx.clone(),
        &tx,
        &&tx,
        &arc,
        &&arc,
        &boxed,
        &Box::new(arc.clone())
    );
}

#[test]
fn nested_slices_stand_in_for_nested_vecm() {
    let inner: [Arc<u32>; 2] = [Arc::new(1), Arc::new(2)];
    let outer: [&[Arc<u32>]; 2] = [&inner[..], &inner[..1]];

    check_stands_in!(
        VecM<VecM<u32, 2>, 3>,
        vec![vec![1, 2].try_into().unwrap(), vec![1].try_into().unwrap()]
            .try_into()
            .unwrap(),
        &outer[..],
    );
}

// The bound is a promise, not a check: an oversized slice still encodes,
// and the receiver is the one that rejects it.
#[test]
fn slice_length_is_not_checked_against_bound() {
    fn encode_batch(batch: &(impl EncodesAs<Batch> + ?Sized)) -> Vec<u8> {
        txs::encode(batch).unwrap()
    }

    let five: Vec<Tx> = (0..5).map(|_| txs()[0].clone()).collect();
    let encoded = encode_batch(&five[..]);

    assert_eq!(encoded, txs::encode(&five).unwrap());
    assert!(txs::decode::<Batch>(&encoded).is_err());
}

#[test]
fn every_write_xdr_type_stands_in_for_itself() {
    fn is_self<T: EncodesAs<T> + ?Sized>() {}

    is_self::<i16>();
    is_self::<i32>();
    is_self::<i64>();
    is_self::<u16>();
    is_self::<u32>();
    is_self::<u64>();
    is_self::<bool>();
    is_self::<f32>();
    is_self::<f64>();
    is_self::<str>();
    is_self::<()>();
    is_self::<core::convert::Infallible>();
    is_self::<String>();

    is_self::<[u8]>();
    is_self::<[u32]>();
    is_self::<[u8; 3]>();
    is_self::<[u32; 3]>();
    is_self::<Vec<u8>>();
    is_self::<Vec<u32>>();
    is_self::<BytesM<4>>();
    is_self::<StringM<4>>();
    is_self::<VecM<u8, 4>>();
    is_self::<VecM<u32, 4>>();

    is_self::<Option<u32>>();
    is_self::<Result<u32, bool>>();
    is_self::<core::marker::PhantomData<u32>>();
    is_self::<(u32,)>();
    is_self::<(u32, bool)>();
    is_self::<(
        [u8; 2],
        u16,
        u32,
        u64,
        i16,
        i32,
        i64,
        bool,
        f32,
        f64,
        (),
        String,
        Tx,
        Batch,
        Option<Tx>,
        Wrap<u32>,
    )>();

    is_self::<Tx>();
    is_self::<Batch>();
    is_self::<Wrap<Tx>>();

    #[cfg(feature = "int128")]
    {
        is_self::<i128>();
        is_self::<u128>();
    }

    #[cfg(feature = "heapless")]
    {
        is_self::<heapless::Vec<u8, 4>>();
        is_self::<heapless::Vec<u32, 4>>();
        is_self::<heapless::String<4>>();
    }
}

// Chunk lengths that are not multiples of 4 exercise TXS padding: the
// chunks must be written back to back with one pad at the end, not padded
// one by one.
#[test]
fn chunked_bytes_stand_in_for_bytesm() {
    let cases: [&[&[u8]]; 8] = [
        &[],
        &[b""],
        &[b"a"],
        &[b"a", b"b"],
        &[b"ab", b"c"],
        &[b"abcd", b"efgh"],
        &[b"abc", b"", b"de", b"f", b"ghij", b"k"],
        &[b"", b"lmnopqr", b""],
    ];

    for chunks in cases {
        let joined: Vec<u8> = chunks.concat();
        let borrowed = ChunkedBytes::<&[u8], 16>(chunks.to_vec());
        let owned = ChunkedBytes::<Vec<u8>, 16>(chunks.iter().map(|c| c.to_vec()).collect());
        let shared = Arc::new(borrowed.clone());

        assert_eq!(borrowed.len(), joined.len());
        assert_eq!(borrowed.is_empty(), joined.is_empty());
        check_stands_in!(
            BytesM<16>,
            BytesM::<16>(joined.clone()),
            &borrowed,
            &owned,
            &shared,
        );
    }
}

#[test]
fn chunked_bytes_share_the_size_bound_of_bytesm() {
    type Chunked = ChunkedBytes<Vec<u8>, 16>;

    assert_eq!(
        <Chunked as cxs::ConstMaxSize>::MAX_SIZE,
        <BytesM<16> as cxs::ConstMaxSize>::MAX_SIZE
    );
    assert_eq!(
        <Chunked as txs::ConstMaxSize>::MAX_SIZE,
        <BytesM<16> as txs::ConstMaxSize>::MAX_SIZE
    );
}

// Like `BytesM`, and unlike a slice standing in for `VecM`, the bound is
// enforced when writing.
#[test]
fn chunked_bytes_reject_more_than_max() {
    let chunked = ChunkedBytes::<&[u8], 4>(vec![b"abc", b"de"]);

    assert!(matches!(
        txs::encode(&chunked),
        Err(WriteError::InvalidArrayLength)
    ));
    assert!(matches!(
        cxs::encode(&chunked),
        Err(WriteError::InvalidArrayLength)
    ));
    assert!(matches!(
        txs::encode(&BytesM::<4>(b"abcde".to_vec())),
        Err(WriteError::InvalidArrayLength)
    ));
}

// A format defined outside xdrs gets `write_opaque_variable_chunks` from
// the trait's default, which writes the concatenation.
#[test]
fn chunked_bytes_use_the_default_for_other_writers() {
    struct Plain;

    #[derive(Default)]
    struct Recorder(Vec<String>);

    impl Writer<Plain> for Recorder {
        fn write_opaque_fixed(&mut self, bytes: &[u8]) -> WriteResult<()> {
            self.0.push(format!("fixed {bytes:?}"));
            Ok(())
        }
        fn write_opaque_variable(&mut self, bytes: &[u8]) -> WriteResult<()> {
            self.0.push(format!("variable {bytes:?}"));
            Ok(())
        }
        fn write_i64(&mut self, n: i64) -> WriteResult<()> {
            self.0.push(format!("i64 {n}"));
            Ok(())
        }
        fn write_i32(&mut self, n: i32) -> WriteResult<()> {
            self.0.push(format!("i32 {n}"));
            Ok(())
        }
        fn write_i16(&mut self, n: i16) -> WriteResult<()> {
            self.0.push(format!("i16 {n}"));
            Ok(())
        }
        fn write_u64(&mut self, n: u64) -> WriteResult<()> {
            self.0.push(format!("u64 {n}"));
            Ok(())
        }
        fn write_u32(&mut self, n: u32) -> WriteResult<()> {
            self.0.push(format!("u32 {n}"));
            Ok(())
        }
        fn write_u16(&mut self, n: u16) -> WriteResult<()> {
            self.0.push(format!("u16 {n}"));
            Ok(())
        }
        fn write_bool(&mut self, b: bool) -> WriteResult<()> {
            self.0.push(format!("bool {b}"));
            Ok(())
        }
        fn write_f32(&mut self, n: f32) -> WriteResult<()> {
            self.0.push(format!("f32 {n}"));
            Ok(())
        }
        fn write_f64(&mut self, n: f64) -> WriteResult<()> {
            self.0.push(format!("f64 {n}"));
            Ok(())
        }
    }

    let mut chunked = Recorder::default();
    ChunkedBytes::<&[u8]>(vec![b"ab", b"", b"c"])
        .write_xdr::<Plain, _>(&mut chunked)
        .unwrap();
    let mut owned = Recorder::default();
    BytesM::<{ u32::MAX }>(b"abc".to_vec())
        .write_xdr::<Plain, _>(&mut owned)
        .unwrap();

    assert_eq!(chunked.0, owned.0);
    assert_eq!(chunked.0, ["variable [97, 98, 99]"]);
}
