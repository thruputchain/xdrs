use rpc_api::CallBuf;

fn call_with(payload: &[u8]) -> CallBuf {
    let mut call = CallBuf::new();
    call.extend_from_slice(payload);
    call
}

#[test]
fn new_has_empty_payload_and_full_prefix() {
    let buf = CallBuf::new();
    assert!(buf.payload().is_empty());
    assert_eq!(buf.into_vec().len(), CallBuf::PREFIX_LEN);
}

#[test]
fn payload_sits_after_prefix() {
    let mut buf = call_with(b"abc");
    buf.extend_from_slice(b"def");
    assert_eq!(buf.payload(), b"abcdef");

    let vec = buf.into_vec();
    assert_eq!(vec.len(), CallBuf::PREFIX_LEN + 6);
    assert_eq!(&vec[CallBuf::PREFIX_LEN..], b"abcdef");
}

#[test]
fn encode_appends_xdr_after_prefix() {
    let mut buf = call_with(b"pre");
    buf.encode(&7u32).unwrap();
    buf.encode(&(1u16, true)).unwrap();
    assert_eq!(
        buf.payload(),
        [
            b"pre".as_slice(),
            &xdrs::txs::encode(&(7u32, (1u16, true))).unwrap()
        ]
        .concat()
    );
}

#[test]
fn from_spare_reuses_allocation_and_reserves_prefix() {
    let spare = Vec::with_capacity(1024);
    let ptr = spare.as_ptr();
    let mut buf = CallBuf::from_spare(spare);
    assert!(buf.payload().is_empty());
    buf.extend_from_slice(&[1; 512]);
    let vec = buf.into_vec();
    assert_eq!(vec.as_ptr(), ptr);
    assert_eq!(vec.len(), CallBuf::PREFIX_LEN + 512);

    let dirty = vec![7; 100];
    assert!(CallBuf::from_spare(dirty).payload().is_empty());
}
