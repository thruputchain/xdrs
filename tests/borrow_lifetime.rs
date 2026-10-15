//! Borrowed types decode for every lifetime the reader outlives, not only
//! the reader's own.
//!
//! By value, a `&'r [u8]` coming out of the reader can be shortened to
//! `&'a [u8]` at the use site. Through an invariant position (a generic
//! parameter, a slot behind `&mut`) it cannot, so the impl itself must
//! accept the shorter lifetime. These functions name both lifetimes
//! explicitly so the shortening has to happen inside the impl.

use xdrs::ReadXDR;
use xdrs::serialize::reader::Reader;
use xdrs::txs;
use xdrs::txs::buf_reader::BufReader;

// Both ends are pinned here. The reader sits behind `&mut`, so `'r` cannot
// be shortened, and `T` is fixed by the `&mut Option<T>` slot, so the bound
// `T: ReadXDR<'r>` is checked for the slot's own type, with no subtyping
// step in between. (Passing the buffer as `&'r [u8]`, or assigning a decoded
// value into the slot directly, would let the compiler shorten something
// along the way and sidestep the question.)
fn decode_into<'r, R: Reader<'r>, T: ReadXDR<'r>>(r: &mut R, slot: &mut Option<T>) {
    *slot = Some(xdrs::read_xdr(r).unwrap());
}

fn fill_slice<'r: 'a, 'a>(r: &mut BufReader<'r>, slot: &mut Option<&'a [u8]>) {
    decode_into(r, slot);
}

fn fill_array<'r: 'a, 'a>(r: &mut BufReader<'r>, slot: &mut Option<&'a [u8; 2]>) {
    decode_into(r, slot);
}

fn fill_str<'r: 'a, 'a>(r: &mut BufReader<'r>, slot: &mut Option<&'a str>) {
    decode_into(r, slot);
}

#[derive(Debug, PartialEq, Eq, xdrs::ReadXDR, xdrs::WriteXDR)]
struct Borrowed<'a> {
    bytes: &'a [u8],
    fixed: &'a [u8; 2],
    text: &'a str,
}

fn fill_struct<'r: 'a, 'a>(r: &mut BufReader<'r>, slot: &mut Option<Borrowed<'a>>) {
    decode_into(r, slot);
}

#[test]
fn borrowed_slice_into_shorter_lifetime_slot() {
    // length 2, then "ab" padded to 4.
    let buf = [0, 0, 0, 2, b'a', b'b', 0, 0];
    let mut slot = None;
    fill_slice(&mut BufReader::new(&buf), &mut slot);
    let s = slot.unwrap();
    assert_eq!(s, b"ab");
    assert_eq!(
        s.as_ptr(),
        buf[4..].as_ptr(),
        "still borrows from the input"
    );
}

#[test]
fn borrowed_array_into_shorter_lifetime_slot() {
    let buf = [b'a', b'b', 0, 0];
    let mut slot = None;
    fill_array(&mut BufReader::new(&buf), &mut slot);
    let a = slot.unwrap();
    assert_eq!(a, b"ab");
    assert_eq!(a.as_ptr(), buf.as_ptr());
}

#[test]
fn borrowed_str_into_shorter_lifetime_slot() {
    let buf = [0, 0, 0, 2, b'a', b'b', 0, 0];
    let mut slot = None;
    fill_str(&mut BufReader::new(&buf), &mut slot);
    let t = slot.unwrap();
    assert_eq!(t, "ab");
    assert_eq!(t.as_ptr(), buf[4..].as_ptr());
}

#[test]
fn derived_struct_into_shorter_lifetime_slot() {
    let value = Borrowed {
        bytes: b"xyz",
        fixed: b"ab",
        text: "hi",
    };
    let buf = txs::encode(&value).unwrap();
    let mut slot = None;
    fill_struct(&mut BufReader::new(&buf), &mut slot);
    assert_eq!(slot.unwrap(), value);
}
