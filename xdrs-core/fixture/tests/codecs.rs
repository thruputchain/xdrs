//! The fixture's types, derived against xdrs-core alone, encode and decode
//! through the codecs in xdrs: the traits xdrs re-exports are xdrs-core's.

use xdrs_core::{BytesM, StringM, VecM};
use xdrs_core_fixture::{Borrowed, ExtensionPoint, Header, Inner, Message, send_header};

fn header() -> Header {
    Header {
        ext: ExtensionPoint::V0,
        hash: [7; 32],
        label: StringM::try_from("fixture").unwrap(),
        data: BytesM::try_from(vec![1, 2, 3]).unwrap(),
        nested: Box::new(Inner { n: 42, flag: true }),
    }
}

#[test]
fn roundtrips_through_both_codecs() {
    let messages = [
        Message::Ping,
        Message::Header(header()),
        Message::Counters(VecM::try_from(vec![1, 2, 3]).unwrap()),
        Message::Other(0x8000_0000),
    ];
    for m in &messages {
        let txs = xdrs::txs::encode(m).unwrap();
        assert_eq!(&xdrs::txs::decode::<Message>(&txs).unwrap(), m);
        let cxs = xdrs::cxs::encode(m).unwrap();
        assert_eq!(&xdrs::cxs::decode::<Message>(&cxs).unwrap(), m);
    }
}

#[test]
fn borrowed_decode_points_into_the_input() {
    let value = Borrowed {
        bytes: &[9, 8, 7],
        text: "hi",
    };
    let txs = xdrs::txs::encode(&value).unwrap();
    let decoded: Borrowed<'_> = xdrs::txs::decode(&txs).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(decoded.bytes.as_ptr(), txs[4..].as_ptr());
}

#[test]
fn const_writers_match_the_runtime_encoders() {
    let inner = Inner { n: 42, flag: true };

    let mut w = xdrs::txs::ConstWriter::<12>::new();
    inner.const_write_txs(&mut w);
    assert_eq!(
        w.into_bytes().as_slice(),
        xdrs::txs::encode(&inner).unwrap()
    );

    // CXS is variable-width: 42 and `true` each fit in a header byte.
    let mut w = xdrs::cxs::ConstWriter::<2>::new();
    inner.const_write_cxs(&mut w);
    let bytes = xdrs::cxs::encode(&inner).unwrap();
    assert_eq!(w.into_bytes().as_slice(), bytes);
    assert!(bytes.len() <= <Inner as xdrs::cxs::ConstMaxSize>::MAX_SIZE.unwrap());
}

#[test]
fn size_bounds_are_the_core_traits() {
    // The same trait, named through either crate.
    fn bound<T: xdrs::txs::ConstMaxSize>() -> Option<usize> {
        <T as xdrs_core::txs::ConstMaxSize>::MAX_SIZE
    }
    assert_eq!(bound::<Inner>(), Some(12));
    // Boxed fields are pointer cuts.
    assert_eq!(bound::<Header>(), None);
}

#[test]
fn rpc_style_signature_takes_xdrs_values() {
    let h = header();
    assert_eq!(send_header(&h), xdrs::txs::encode(&h).unwrap().len());
}
