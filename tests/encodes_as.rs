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
    let boxed_slice: Box<[Tx]> = owned.to_vec().into();
    let boxed_arcs: Box<[Arc<Tx>]> = arcs.clone().into();
    let boxed_refs: Box<[&Tx]> = refs.clone().into();

    check_stands_in!(
        Batch,
        Batch::try_from(owned.to_vec()).unwrap(),
        &owned[..],
        &arcs[..],
        &refs[..],
        &arc_refs[..],
        &boxes[..],
        &shared_slice,
        &boxed_slice,
        &boxed_arcs,
        &boxed_refs,
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

    let boxed: Box<[Box<[Arc<u32>]>]> =
        Box::new([inner.to_vec().into(), inner[..1].to_vec().into()]);
    check_stands_in!(
        VecM<VecM<u32, 2>, 3>,
        vec![vec![1, 2].try_into().unwrap(), vec![1].try_into().unwrap()]
            .try_into()
            .unwrap(),
        &boxed,
    );
}

// A value returned as `impl EncodesAs<X>`, as an RPC server handler's reply
// is: boxed slices are built and handed back by value.
#[test]
fn boxed_slices_stand_in_by_value() {
    fn reply(txs: &[Tx; 2]) -> impl EncodesAs<Batch> {
        txs.iter().cloned().map(Arc::new).collect::<Box<[_]>>()
    }
    fn borrowed_reply(txs: &[Tx; 2]) -> impl EncodesAs<Batch> + '_ {
        txs.iter().collect::<Box<[&Tx]>>()
    }

    let owned = txs();
    let expected = Batch::try_from(owned.to_vec()).unwrap();
    check_stands_in!(Batch, expected.clone(), &reply(&owned));
    check_stands_in!(Batch, expected.clone(), &borrowed_reply(&owned));
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
    is_self::<ChunkedBytes<Vec<Vec<u8>>, 4>>();
    is_self::<ChunkedVec<Vec<Vec<u32>>, 4>>();

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
    let cases: [&[&[u8]]; 11] = [
        &[],
        &[b""],
        &[b"a"],
        &[b"a", b"b"],
        &[b"ab", b"c"],
        &[b"abcd", b"efgh"],
        &[b"abc", b"", b"de", b"f", b"ghij", b"k"],
        &[b"", b"lmnopqr", b""],
        &[b"a", b"b", b"c", b"d", b"e"],
        &[b"a", b"bcdefghijk", b"lm"],
        &[b"abcdefg", b"hijkl", b"m", b"no"],
    ];

    for chunks in cases {
        let joined: Vec<u8> = chunks.concat();
        let borrowed = ChunkedBytes::<_, 16>(chunks);
        let owned = ChunkedBytes::<_, 16>(chunks.iter().map(|c| c.to_vec()).collect::<Vec<_>>());
        let shared_chunks = ChunkedBytes::<_, 16>(
            chunks
                .iter()
                .map(|&c| Arc::<[u8]>::from(c))
                .collect::<Arc<[_]>>(),
        );
        let shared = Arc::new(borrowed);

        assert_eq!(owned.len(), joined.len());
        assert_eq!(owned.is_empty(), joined.is_empty());
        check_stands_in!(
            BytesM<16>,
            BytesM::<16>(joined.clone()),
            &borrowed,
            &owned,
            &shared_chunks,
            &shared,
        );
    }
}

#[test]
fn chunked_vec_stands_in_for_vecm() {
    let [one, two] = txs();
    let cases: [Vec<Vec<Tx>>; 6] = [
        vec![],
        vec![vec![]],
        vec![vec![one.clone()]],
        vec![vec![one.clone(), two.clone()]],
        vec![vec![one.clone()], vec![], vec![two.clone()]],
        vec![
            vec![two.clone(), one.clone()],
            vec![one.clone(), two.clone()],
        ],
    ];

    for chunks in cases {
        let joined: Vec<Tx> = chunks.concat();
        let owned = ChunkedVec::<_, 4>(chunks.clone());
        let borrowed: Vec<&[Tx]> = chunks.iter().map(Vec::as_slice).collect();
        let borrowed = ChunkedVec::<_, 4>(&borrowed[..]);
        let shared_elements = ChunkedVec::<_, 4>(
            chunks
                .iter()
                .map(|c| c.iter().cloned().map(Arc::new).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        );
        let shared = Arc::new(owned.clone());

        assert_eq!(owned.len(), joined.len());
        assert_eq!(owned.is_empty(), joined.is_empty());
        check_stands_in!(
            Batch,
            Batch::try_from(joined.clone()).unwrap(),
            &owned,
            &borrowed,
            &shared_elements,
            &shared,
        );
    }
}

#[test]
fn chunked_types_share_the_size_bound_of_their_wire_types() {
    type Bytes = ChunkedBytes<Vec<Vec<u8>>, 16>;
    type Elements = ChunkedVec<Vec<Vec<u32>>, 16>;

    assert_eq!(
        <Bytes as cxs::ConstMaxSize>::MAX_SIZE,
        <BytesM<16> as cxs::ConstMaxSize>::MAX_SIZE
    );
    assert_eq!(
        <Bytes as txs::ConstMaxSize>::MAX_SIZE,
        <BytesM<16> as txs::ConstMaxSize>::MAX_SIZE
    );
    assert_eq!(
        <Elements as cxs::ConstMaxSize>::MAX_SIZE,
        <VecM<u32, 16> as cxs::ConstMaxSize>::MAX_SIZE
    );
    assert_eq!(
        <Elements as txs::ConstMaxSize>::MAX_SIZE,
        <VecM<u32, 16> as txs::ConstMaxSize>::MAX_SIZE
    );
}
