#[test]
#[ignore]
fn txs_bufwriter_variable_opaque_near_u32_max() {
    let payload_len: usize = u32::MAX as usize - 3;
    let payload = vec![0u8; payload_len];

    let encoded = xdrs::txs::encode(payload.as_slice()).unwrap();

    let expected_total = 4 + payload_len;
    assert_eq!(encoded.len(), expected_total);

    let length_prefix = u32::from_be_bytes(encoded[..4].try_into().unwrap());
    assert_eq!(length_prefix, payload_len as u32);
}
