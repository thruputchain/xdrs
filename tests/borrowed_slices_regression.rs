//! Regression tests for deriving `ReadXDR` on structs holding `&'a [u8]` and
//! other borrowed shapes.
//!
//! Originally tracked the E0283 bug introduced by PR #220 on structs with
//! multiple distinct lifetime parameters each used in a `&[u8]` field. That
//! PR has been reverted; the derive now always emits `'_r: 'lt` for every
//! lifetime parameter `'lt` on the struct (see the `ReadXDR` docs for the
//! buffer-lifetime constraint that follows). These tests cover the shapes
//! that were broken under PR #220 and confirm they decode correctly now.

mod common;
use common::*;

// --- Single-lifetime borrowed shapes ----------------------------------------

// Tuple struct holding only `&'a [u8]` — minimal newtype shape.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct TupleBytes<'a>(&'a [u8]);

#[test]
fn tuple_struct_bytes_only() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val = TupleBytes(&bytes);
    CXS::check_codec(&val, &hex!("03aabbcc"));
    TXS::check_codec(&val, &hex!("00000003aabbcc00"));
}

// Named single-field struct holding only `&'a [u8]`.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct SingleBytes<'a> {
    bytes: &'a [u8],
}

#[test]
fn single_field_struct_bytes_only() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val = SingleBytes { bytes: &bytes };
    CXS::check_codec(&val, &hex!("03aabbcc"));
    TXS::check_codec(&val, &hex!("00000003aabbcc00"));
}

// Two `&'a [u8]` fields sharing one lifetime.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct DoubleBytes<'a> {
    a: &'a [u8],
    b: &'a [u8],
}

#[test]
fn double_bytes_struct() {
    let a = [0xaau8];
    let b = [0xbbu8, 0xcc];
    let val = DoubleBytes { a: &a, b: &b };
    CXS::check_codec(&val, &hex!("01aa02bbcc"));
    TXS::check_codec(&val, &hex!("00000001aa00000000000002bbcc0000"));
}

// Borrowed slice followed by a non-borrowed sibling (reverse field order).
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct BytesThenTag<'a> {
    bytes: &'a [u8],
    tag: u32,
}

#[test]
fn bytes_then_tag() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val = BytesThenTag {
        bytes: &bytes,
        tag: 7,
    };
    CXS::check_codec(&val, &hex!("03aabbcc07"));
    TXS::check_codec(&val, &hex!("00000003aabbcc0000000007"));
}

// --- Multi-lifetime borrowed shapes -----------------------------------------

// Two distinct lifetime parameters, each used by a `&[u8]` field.
//
// This is the shape that fails in sdk_rs (`MultiLifetimeEvent<'a, 'b>` after
// the `#[derive(Event)]` macro generates the payload struct). PR #220's
// per-field `where T: ReadXDR<'_r>` bounds tripped E0283 here because the
// where clause forced T to be `&'a [u8]` while the blanket
// `impl<'r> ReadXDR<'r> for &'r [u8]` required `'a = '_r` — and the solver
// couldn't reconcile two such locks under distinct lifetimes. With the
// unconditional `'_r: 'a + 'b` outlives bound restored, both lifetimes
// collapse onto the buffer's lifetime at the call site and the blanket
// impls apply.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct TwoLifetimeBytes<'a, 'b> {
    a: &'a [u8],
    b: &'b [u8],
}

#[test]
fn two_lifetime_bytes() {
    let a = [0xaau8];
    let b = [0xbbu8, 0xcc];
    let val = TwoLifetimeBytes { a: &a, b: &b };
    CXS::check_codec(&val, &hex!("01aa02bbcc"));
    TXS::check_codec(&val, &hex!("00000001aa00000000000002bbcc0000"));
}

// Borrowed `'a` next to an "extra" `'b` that's only used in a PhantomData.
// All lifetimes default-bind to the reader buffer's lifetime; the phantom
// lifetime cannot be chosen to outlive the buffer.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct MixedBorrowedAndPhantom<'a, 'b> {
    bytes: &'a [u8],
    phantom: core::marker::PhantomData<&'b ()>,
}

#[test]
fn mixed_borrowed_and_phantom() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val: MixedBorrowedAndPhantom<'_, '_> = MixedBorrowedAndPhantom {
        bytes: &bytes,
        phantom: core::marker::PhantomData,
    };
    CXS::check_codec(&val, &hex!("03aabbcc"));
    TXS::check_codec(&val, &hex!("00000003aabbcc00"));
}

// Three lifetimes mixed: two borrowed (different) + one phantom-only.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct ThreeLifetimes<'a, 'b, 'c> {
    a: &'a [u8],
    b: &'b [u8],
    phantom: core::marker::PhantomData<&'c ()>,
}

#[test]
fn three_lifetimes_two_borrowed_one_phantom() {
    let a = [0xaau8];
    let b = [0xbbu8, 0xcc];
    let val: ThreeLifetimes<'_, '_, '_> = ThreeLifetimes {
        a: &a,
        b: &b,
        phantom: core::marker::PhantomData,
    };
    CXS::check_codec(&val, &hex!("01aa02bbcc"));
    TXS::check_codec(&val, &hex!("00000001aa00000000000002bbcc0000"));
}

// A struct whose only generic-parameter usage is via another struct that
// happens to hold a PhantomData. The outer `'a` still propagates the
// `'_r: 'a` bound and the value can be decoded so long as 'a matches the
// buffer's lifetime.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct PhantomWrapper<'a> {
    phantom: core::marker::PhantomData<&'a u32>,
}

#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct OuterOverPhantomWrapper<'a> {
    wrap: PhantomWrapper<'a>,
    value: u32,
}

#[test]
fn outer_over_phantom_wrapper_decodes_when_lifetimes_match() {
    let bytes = hex!("00000007");
    let v: OuterOverPhantomWrapper<'_> = xdrs::txs::decode(&bytes).unwrap();
    assert_eq!(v.value, 7);
}

// Borrowed `&'a [u8]` next to a wrapper that itself carries a lifetime.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct MixedThroughWrapper<'a, 'b> {
    bytes: &'a [u8],
    phantom_wrap: PhantomWrapper<'b>,
}

#[test]
fn mixed_through_wrapper_decodes_when_lifetimes_match() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val: MixedThroughWrapper<'_, '_> = MixedThroughWrapper {
        bytes: &bytes,
        phantom_wrap: PhantomWrapper {
            phantom: core::marker::PhantomData,
        },
    };
    CXS::check_codec(&val, &hex!("03aabbcc"));
    TXS::check_codec(&val, &hex!("00000003aabbcc00"));
}

// --- Enum and attribute coverage --------------------------------------------

// Enum variant carrying a `&'a [u8]` field.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
#[repr(i32)]
enum BytesEnum<'a> {
    Bytes(&'a [u8]) = 0,
    Tag(u32) = 1,
}

#[test]
fn enum_variant_bytes() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val = BytesEnum::Bytes(&bytes);
    CXS::check_codec(&val, &hex!("0003aabbcc"));
    TXS::check_codec(&val, &hex!("0000000000000003aabbcc00"));
}

// Input-level `#[xdr(from_type = "&'a [u8]")]` newtype.
//
// This exercises `read_from_type` (the input-level from_type path) instead
// of the per-field path used by structs.
#[derive(Debug, PartialEq, Eq, Clone, xdrs::WriteXDR, xdrs::ReadXDR)]
#[xdr(from_type = "&'a [u8]", into_type = "&'a [u8]")]
struct ViaBytes<'a>(&'a [u8]);

impl<'a> From<&'a [u8]> for ViaBytes<'a> {
    fn from(b: &'a [u8]) -> Self {
        ViaBytes(b)
    }
}

impl<'a> From<ViaBytes<'a>> for &'a [u8] {
    fn from(v: ViaBytes<'a>) -> Self {
        v.0
    }
}

#[test]
fn input_level_from_type_bytes() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val = ViaBytes(&bytes);
    CXS::check_codec(&val, &hex!("03aabbcc"));
    TXS::check_codec(&val, &hex!("00000003aabbcc00"));
}

// Field-level `#[xdr(from_type = "&'a [u8]")]`.
#[derive(Debug, PartialEq, Eq, xdrs::WriteXDR, xdrs::ReadXDR)]
struct FieldFromTypeBytes<'a> {
    #[xdr(from_type = "&'a [u8]", into_type = "&'a [u8]")]
    bytes: &'a [u8],
}

#[test]
fn field_level_from_type_bytes() {
    let bytes = [0xaa, 0xbb, 0xcc];
    let val = FieldFromTypeBytes { bytes: &bytes };
    CXS::check_codec(&val, &hex!("03aabbcc"));
    TXS::check_codec(&val, &hex!("00000003aabbcc00"));
}
