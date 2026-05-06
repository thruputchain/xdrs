//! Tests that const writers produce the same output as runtime encoders.
#![allow(dead_code, unused_imports)]

/// Helper: runtime-encode a value with CXS, return the bytes.
fn cxs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::cxs::encode(val).unwrap()
}

/// Helper: runtime-encode a value with TXS, return the bytes.
fn txs_runtime_encode<T: xdrs::WriteXDR>(val: &T) -> Vec<u8> {
    xdrs::txs::encode(val).unwrap()
}

// -- TXS cross-validation --

const TXS_I32_NEG1: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    w.write_i32(-1);
    w.into_bytes()
};

const TXS_U64_DEADBEEF: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    w.write_u64(0xdead1234beef5678);
    w.into_bytes()
};

const TXS_OPAQUE_FIXED_3: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    w.write_opaque_fixed(&[0xAA, 0xBB, 0xCC]);
    w.into_bytes()
};

#[test]
fn txs_cross_validate_i32() {
    assert_eq!(&TXS_I32_NEG1, txs_runtime_encode(&-1i32).as_slice());
}

#[test]
fn txs_cross_validate_u64() {
    assert_eq!(
        &TXS_U64_DEADBEEF,
        txs_runtime_encode(&0xdead1234beef5678u64).as_slice()
    );
}

#[test]
fn txs_cross_validate_opaque_fixed() {
    let bytes: [u8; 3] = [0xAA, 0xBB, 0xCC];
    assert_eq!(&TXS_OPAQUE_FIXED_3, txs_runtime_encode(&bytes).as_slice());
}

// -- CXS cross-validation --

const CXS_I32_0: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    w.write_i32(0);
    w.into_bytes()
};

const CXS_I32_MAX: [u8; 5] = {
    let mut w = xdrs::cxs::ConstWriter::<5>::new();
    w.write_i32(i32::MAX);
    w.into_bytes()
};

const CXS_U32_DEADBEEF: [u8; 5] = {
    let mut w = xdrs::cxs::ConstWriter::<5>::new();
    w.write_u32(0xdeadbeef);
    w.into_bytes()
};

const CXS_I64_MAX: [u8; 9] = {
    let mut w = xdrs::cxs::ConstWriter::<9>::new();
    w.write_i64(i64::MAX);
    w.into_bytes()
};

const CXS_U64_DB: [u8; 9] = {
    let mut w = xdrs::cxs::ConstWriter::<9>::new();
    w.write_u64(0xdead1234beef5678);
    w.into_bytes()
};

#[test]
fn cxs_cross_validate_i32_zero() {
    assert_eq!(&CXS_I32_0, cxs_runtime_encode(&0i32).as_slice());
}

#[test]
fn cxs_cross_validate_i32_max() {
    assert_eq!(&CXS_I32_MAX, cxs_runtime_encode(&i32::MAX).as_slice());
}

#[test]
fn cxs_cross_validate_u32_deadbeef() {
    assert_eq!(
        &CXS_U32_DEADBEEF,
        cxs_runtime_encode(&0xdeadbeefu32).as_slice()
    );
}

#[test]
fn cxs_cross_validate_i64_max() {
    assert_eq!(&CXS_I64_MAX, cxs_runtime_encode(&i64::MAX).as_slice());
}

#[test]
fn cxs_cross_validate_u64_db() {
    assert_eq!(
        &CXS_U64_DB,
        cxs_runtime_encode(&0xdead1234beef5678u64).as_slice()
    );
}

// -- Derive macro tests: structs --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Foo {
    bytes: [u8; 3],
    blerg: i32,
}

const FOO: Foo = Foo {
    bytes: [0xAA, 0xBB, 0xCC],
    blerg: 7,
};

const FOO_CXS: [u8; 4] = {
    let mut w = xdrs::cxs::ConstWriter::<4>::new();
    FOO.const_write_cxs(&mut w);
    w.into_bytes()
};

const FOO_TXS: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    FOO.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_struct_cxs() {
    assert_eq!(&FOO_CXS, cxs_runtime_encode(&FOO).as_slice());
}

#[test]
fn derive_struct_txs() {
    assert_eq!(&FOO_TXS, txs_runtime_encode(&FOO).as_slice());
}

// -- Derive macro tests: enums --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum Status {
    On,
    Off,
}

const STATUS_ON_CXS: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    Status::On.const_write_cxs(&mut w);
    w.into_bytes()
};

const STATUS_ON_TXS: [u8; 4] = {
    let mut w = xdrs::txs::ConstWriter::<4>::new();
    Status::On.const_write_txs(&mut w);
    w.into_bytes()
};

const STATUS_OFF_CXS: [u8; 1] = {
    let mut w = xdrs::cxs::ConstWriter::<1>::new();
    Status::Off.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_enum_bare() {
    assert_eq!(&STATUS_ON_CXS, cxs_runtime_encode(&Status::On).as_slice());
    assert_eq!(&STATUS_ON_TXS, txs_runtime_encode(&Status::On).as_slice());
    assert_eq!(&STATUS_OFF_CXS, cxs_runtime_encode(&Status::Off).as_slice());
}

// Enum with fields
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
enum Shape {
    Circle { radius: u32 } = 0,
    Rect { w: u32, h: u32 } = 1,
    Empty = 2,
}

const CIRCLE_CXS: [u8; 2] = {
    let mut w = xdrs::cxs::ConstWriter::<2>::new();
    (Shape::Circle { radius: 5 }).const_write_cxs(&mut w);
    w.into_bytes()
};

const RECT_TXS: [u8; 12] = {
    let mut w = xdrs::txs::ConstWriter::<12>::new();
    (Shape::Rect { w: 10, h: 20 }).const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_enum_with_fields() {
    let circle = Shape::Circle { radius: 5 };
    assert_eq!(&CIRCLE_CXS, cxs_runtime_encode(&circle).as_slice());

    let rect = Shape::Rect { w: 10, h: 20 };
    assert_eq!(&RECT_TXS, txs_runtime_encode(&rect).as_slice());
}

// Nested: struct containing enum
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Config {
    version: u32,
    status: Status,
}

const CONFIG: Config = Config {
    version: 1,
    status: Status::On,
};

const CONFIG_CXS: [u8; 2] = {
    let mut w = xdrs::cxs::ConstWriter::<2>::new();
    CONFIG.const_write_cxs(&mut w);
    w.into_bytes()
};

const CONFIG_TXS: [u8; 8] = {
    let mut w = xdrs::txs::ConstWriter::<8>::new();
    CONFIG.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_nested_struct_enum() {
    assert_eq!(&CONFIG_CXS, cxs_runtime_encode(&CONFIG).as_slice());
    assert_eq!(&CONFIG_TXS, txs_runtime_encode(&CONFIG).as_slice());
}

// -- Size calculation tests --

// Verify size methods match actual encoded sizes
#[test]
fn size_matches_encoded_struct() {
    assert_eq!(FOO.const_cxs_size(), cxs_runtime_encode(&FOO).len());
    assert_eq!(FOO.const_txs_size(), txs_runtime_encode(&FOO).len());
    assert_eq!(CONFIG.const_cxs_size(), cxs_runtime_encode(&CONFIG).len());
    assert_eq!(CONFIG.const_txs_size(), txs_runtime_encode(&CONFIG).len());
}

#[test]
fn size_matches_encoded_enum() {
    let on = Status::On;
    let off = Status::Off;
    assert_eq!(on.const_cxs_size(), cxs_runtime_encode(&on).len());
    assert_eq!(on.const_txs_size(), txs_runtime_encode(&on).len());
    assert_eq!(off.const_cxs_size(), cxs_runtime_encode(&off).len());

    let circle = Shape::Circle { radius: 5 };
    let rect = Shape::Rect { w: 10, h: 20 };
    let empty = Shape::Empty;
    assert_eq!(circle.const_cxs_size(), cxs_runtime_encode(&circle).len());
    assert_eq!(circle.const_txs_size(), txs_runtime_encode(&circle).len());
    assert_eq!(rect.const_cxs_size(), cxs_runtime_encode(&rect).len());
    assert_eq!(rect.const_txs_size(), txs_runtime_encode(&rect).len());
    assert_eq!(empty.const_cxs_size(), cxs_runtime_encode(&empty).len());
    assert_eq!(empty.const_txs_size(), txs_runtime_encode(&empty).len());
}

// The main use case: use const_*_size to compute N automatically
const AUTO_TXS: [u8; CONFIG.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ CONFIG.const_txs_size() }>::new();
    CONFIG.const_write_txs(&mut w);
    w.into_bytes()
};

const AUTO_CXS: [u8; CONFIG.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ CONFIG.const_cxs_size() }>::new();
    CONFIG.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn auto_sized_encode() {
    assert_eq!(&AUTO_TXS, txs_runtime_encode(&CONFIG).as_slice());
    assert_eq!(&AUTO_CXS, cxs_runtime_encode(&CONFIG).as_slice());
}

// -- CXS variable-width size tests --
// Verify that CXS size calculation handles the cascading encoding sizes correctly.

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct WidthTest {
    small: i32,  // fits in 1 byte (header-only) for small values
    medium: u32, // needs 2+ bytes for values > 62
    big: u32,    // needs 5 bytes for values > u16::MAX
}

const NARROW: WidthTest = WidthTest {
    small: 0,
    medium: 1,
    big: 100_000,
};

const WIDE: WidthTest = WidthTest {
    small: i32::MIN,
    medium: u32::MAX,
    big: 0,
};

#[test]
fn cxs_size_varies_with_value() {
    // Small values → compact encoding
    assert_eq!(NARROW.const_cxs_size(), cxs_runtime_encode(&NARROW).len());
    // Large values → wider encoding
    assert_eq!(WIDE.const_cxs_size(), cxs_runtime_encode(&WIDE).len());
    // CXS sizes should differ because values differ
    assert_ne!(NARROW.const_cxs_size(), WIDE.const_cxs_size());
    // TXS sizes are always the same regardless of value
    assert_eq!(NARROW.const_txs_size(), WIDE.const_txs_size());
}

// Auto-size with variable-width CXS values
const NARROW_CXS: [u8; NARROW.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ NARROW.const_cxs_size() }>::new();
    NARROW.const_write_cxs(&mut w);
    w.into_bytes()
};

const WIDE_CXS: [u8; WIDE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WIDE.const_cxs_size() }>::new();
    WIDE.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn auto_sized_cxs_variable_width() {
    assert_eq!(&NARROW_CXS, cxs_runtime_encode(&NARROW).as_slice());
    assert_eq!(&WIDE_CXS, cxs_runtime_encode(&WIDE).as_slice());
}

// -- Enum variant size divergence (CXS) --
// Different variants encode to different sizes in CXS.

#[test]
fn enum_variant_sizes_differ_cxs() {
    let circle = Shape::Circle { radius: 5 };
    let rect = Shape::Rect { w: 10, h: 20 };
    let empty = Shape::Empty;
    // Circle: discriminant(1) + u32(1) = 2 bytes in CXS
    // Rect: discriminant(1) + u32(1) + u32(1) = 3 bytes in CXS
    // Empty: discriminant(1) = 1 byte in CXS
    assert!(circle.const_cxs_size() < rect.const_cxs_size());
    assert!(empty.const_cxs_size() < circle.const_cxs_size());

    // But all match runtime
    assert_eq!(circle.const_cxs_size(), cxs_runtime_encode(&circle).len());
    assert_eq!(rect.const_cxs_size(), cxs_runtime_encode(&rect).len());
    assert_eq!(empty.const_cxs_size(), cxs_runtime_encode(&empty).len());
}

// -- Struct with opaque data --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct WithOpaque {
    tag: u32,
    data: [u8; 7],
}

const OPAQUE_STRUCT: WithOpaque = WithOpaque {
    tag: 42,
    data: [1, 2, 3, 4, 5, 6, 7],
};

const OPAQUE_STRUCT_TXS: [u8; OPAQUE_STRUCT.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ OPAQUE_STRUCT.const_txs_size() }>::new();
    OPAQUE_STRUCT.const_write_txs(&mut w);
    w.into_bytes()
};

const OPAQUE_STRUCT_CXS: [u8; OPAQUE_STRUCT.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ OPAQUE_STRUCT.const_cxs_size() }>::new();
    OPAQUE_STRUCT.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn struct_with_opaque_auto_sized() {
    assert_eq!(
        &OPAQUE_STRUCT_TXS,
        txs_runtime_encode(&OPAQUE_STRUCT).as_slice()
    );
    assert_eq!(
        &OPAQUE_STRUCT_CXS,
        cxs_runtime_encode(&OPAQUE_STRUCT).as_slice()
    );
}

// -- End-to-end: link_section pattern --
// This is the actual motivating use case.

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR, xdrs::ReadXDR)]
#[repr(i32)]
enum SpecVersion {
    V1 = 0,
}

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR, xdrs::ReadXDR)]
struct InterfaceSpec {
    version: SpecVersion,
    magic: u32,
    flags: u32,
}

const MY_SPEC: InterfaceSpec = InterfaceSpec {
    version: SpecVersion::V1,
    magic: 0xDEAD,
    flags: 1,
};

#[used]
#[cfg_attr(target_family = "wasm", unsafe(link_section = "groundhogspecrawv0"))]
static RAW_INTERFACE_SPEC: [u8; MY_SPEC.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ MY_SPEC.const_txs_size() }>::new();
    MY_SPEC.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn link_section_pattern() {
    // The static exists and matches runtime encoding
    assert_eq!(&RAW_INTERFACE_SPEC, txs_runtime_encode(&MY_SPEC).as_slice());
    // And can be decoded back
    let decoded: InterfaceSpec = xdrs::txs::decode(&RAW_INTERFACE_SPEC).unwrap();
    assert_eq!(decoded.magic, 0xDEAD);
    assert_eq!(decoded.flags, 1);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Leaf {
    code: u32,
}

const LEAF_A: Leaf = Leaf { code: 7 };
const LEAF_B: Leaf = Leaf { code: 42 };
const LEAF_ITEMS: [Leaf; 2] = [LEAF_A, LEAF_B];

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct RecursiveShapes {
    maybe: Option<Leaf>,
    nested: Option<Option<Leaf>>,
    choice: Result<Leaf, Leaf>,
    items: [Leaf; 2],
    pair: (Leaf, Leaf),
}

const RECURSIVE: RecursiveShapes = RecursiveShapes {
    maybe: Some(LEAF_A),
    nested: Some(Some(LEAF_B)),
    choice: Err(LEAF_A),
    items: LEAF_ITEMS,
    pair: (LEAF_A, LEAF_B),
};

const RECURSIVE_TXS: [u8; RECURSIVE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ RECURSIVE.const_txs_size() }>::new();
    RECURSIVE.const_write_txs(&mut w);
    w.into_bytes()
};

const RECURSIVE_CXS: [u8; RECURSIVE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ RECURSIVE.const_cxs_size() }>::new();
    RECURSIVE.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_recursive_concrete_shapes() {
    assert_eq!(&RECURSIVE_TXS, txs_runtime_encode(&RECURSIVE).as_slice());
    assert_eq!(&RECURSIVE_CXS, cxs_runtime_encode(&RECURSIVE).as_slice());
    assert_eq!(
        RECURSIVE.const_txs_size(),
        txs_runtime_encode(&RECURSIVE).len()
    );
    assert_eq!(
        RECURSIVE.const_cxs_size(),
        cxs_runtime_encode(&RECURSIVE).len()
    );
}

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct NonCopyLeaf {
    code: u32,
}

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct NonCopyTopLevel {
    maybe: Option<NonCopyLeaf>,
    choice: Result<NonCopyLeaf, NonCopyLeaf>,
}

const NON_COPY: NonCopyTopLevel = NonCopyTopLevel {
    maybe: Some(NonCopyLeaf { code: 11 }),
    choice: Err(NonCopyLeaf { code: 99 }),
};

const NON_COPY_TXS: [u8; NON_COPY.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ NON_COPY.const_txs_size() }>::new();
    NON_COPY.const_write_txs(&mut w);
    w.into_bytes()
};

const NON_COPY_CXS: [u8; NON_COPY.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ NON_COPY.const_cxs_size() }>::new();
    NON_COPY.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn derive_top_level_option_and_result_for_non_copy_inner() {
    assert_eq!(&NON_COPY_TXS, txs_runtime_encode(&NON_COPY).as_slice());
    assert_eq!(&NON_COPY_CXS, cxs_runtime_encode(&NON_COPY).as_slice());
    assert_eq!(
        NON_COPY.const_txs_size(),
        txs_runtime_encode(&NON_COPY).len()
    );
    assert_eq!(
        NON_COPY.const_cxs_size(),
        cxs_runtime_encode(&NON_COPY).len()
    );
}

// -- Newtype tests --
// Verify that newtype wrappers around primitives work, both as standalone
// types and when used as fields in other derived structs.

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Amount(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Tag(i32);

const AMOUNT_42: Amount = Amount(42);
const TAG_NEG: Tag = Tag(-7);

const AMOUNT_CXS: [u8; AMOUNT_42.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ AMOUNT_42.const_cxs_size() }>::new();
    AMOUNT_42.const_write_cxs(&mut w);
    w.into_bytes()
};

const TAG_TXS: [u8; TAG_NEG.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TAG_NEG.const_txs_size() }>::new();
    TAG_NEG.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn newtype_primitive_standalone() {
    assert_eq!(&AMOUNT_CXS, cxs_runtime_encode(&AMOUNT_42).as_slice());
    assert_eq!(&TAG_TXS, txs_runtime_encode(&TAG_NEG).as_slice());
    assert_eq!(
        AMOUNT_42.const_cxs_size(),
        cxs_runtime_encode(&AMOUNT_42).len()
    );
    assert_eq!(
        AMOUNT_42.const_txs_size(),
        txs_runtime_encode(&AMOUNT_42).len()
    );
    assert_eq!(TAG_NEG.const_cxs_size(), cxs_runtime_encode(&TAG_NEG).len());
    assert_eq!(TAG_NEG.const_txs_size(), txs_runtime_encode(&TAG_NEG).len());
}

// Newtype used as a field in another struct — exercises the fallback path
// where the derive sees "Amount" (not "u64") and calls .const_write_cxs().
#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct Transfer {
    from: Tag,
    to: Tag,
    amount: Amount,
}

const TRANSFER: Transfer = Transfer {
    from: Tag(1),
    to: Tag(2),
    amount: Amount(1_000_000),
};

const TRANSFER_TXS: [u8; TRANSFER.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TRANSFER.const_txs_size() }>::new();
    TRANSFER.const_write_txs(&mut w);
    w.into_bytes()
};

const TRANSFER_CXS: [u8; TRANSFER.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TRANSFER.const_cxs_size() }>::new();
    TRANSFER.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn newtype_as_field() {
    assert_eq!(&TRANSFER_TXS, txs_runtime_encode(&TRANSFER).as_slice());
    assert_eq!(&TRANSFER_CXS, cxs_runtime_encode(&TRANSFER).as_slice());
    assert_eq!(
        TRANSFER.const_txs_size(),
        txs_runtime_encode(&TRANSFER).len()
    );
    assert_eq!(
        TRANSFER.const_cxs_size(),
        cxs_runtime_encode(&TRANSFER).len()
    );
}

// Newtypes inside composite types (Option, array, tuple)
#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct NewtypeComposites {
    maybe: Option<Amount>,
    items: [Tag; 2],
    pair: (Tag, Amount),
}

const NT_COMPOSITES: NewtypeComposites = NewtypeComposites {
    maybe: Some(Amount(99)),
    items: [Tag(10), Tag(20)],
    pair: (Tag(3), Amount(500)),
};

const NT_COMPOSITES_TXS: [u8; NT_COMPOSITES.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ NT_COMPOSITES.const_txs_size() }>::new();
    NT_COMPOSITES.const_write_txs(&mut w);
    w.into_bytes()
};

const NT_COMPOSITES_CXS: [u8; NT_COMPOSITES.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ NT_COMPOSITES.const_cxs_size() }>::new();
    NT_COMPOSITES.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn newtype_in_composites() {
    assert_eq!(
        &NT_COMPOSITES_TXS,
        txs_runtime_encode(&NT_COMPOSITES).as_slice()
    );
    assert_eq!(
        &NT_COMPOSITES_CXS,
        cxs_runtime_encode(&NT_COMPOSITES).as_slice()
    );
    assert_eq!(
        NT_COMPOSITES.const_txs_size(),
        txs_runtime_encode(&NT_COMPOSITES).len()
    );
    assert_eq!(
        NT_COMPOSITES.const_cxs_size(),
        cxs_runtime_encode(&NT_COMPOSITES).len()
    );
}

// -- i128/u128 tests --

#[cfg(feature = "int128")]
mod int128_const {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
    struct Wide {
        a: u128,
        b: i128,
    }

    const WIDE_VAL: Wide = Wide {
        a: 0x0001_dead_1234_beef_5678,
        b: -1,
    };

    const WIDE_TXS: [u8; WIDE_VAL.const_txs_size()] = {
        let mut w = xdrs::txs::ConstWriter::<{ WIDE_VAL.const_txs_size() }>::new();
        WIDE_VAL.const_write_txs(&mut w);
        w.into_bytes()
    };

    const WIDE_CXS: [u8; WIDE_VAL.const_cxs_size()] = {
        let mut w = xdrs::cxs::ConstWriter::<{ WIDE_VAL.const_cxs_size() }>::new();
        WIDE_VAL.const_write_cxs(&mut w);
        w.into_bytes()
    };

    #[test]
    fn i128_u128_const_matches_runtime() {
        assert_eq!(&WIDE_TXS, txs_runtime_encode(&WIDE_VAL).as_slice());
        assert_eq!(&WIDE_CXS, cxs_runtime_encode(&WIDE_VAL).as_slice());
        assert_eq!(
            WIDE_VAL.const_txs_size(),
            txs_runtime_encode(&WIDE_VAL).len()
        );
        assert_eq!(
            WIDE_VAL.const_cxs_size(),
            cxs_runtime_encode(&WIDE_VAL).len()
        );
    }

    // Boundary values
    const U128_ZERO: Wide = Wide { a: 0, b: 0 };
    const U128_MAX: Wide = Wide {
        a: u128::MAX,
        b: i128::MIN,
    };

    const U128_ZERO_TXS: [u8; U128_ZERO.const_txs_size()] = {
        let mut w = xdrs::txs::ConstWriter::<{ U128_ZERO.const_txs_size() }>::new();
        U128_ZERO.const_write_txs(&mut w);
        w.into_bytes()
    };

    const U128_MAX_TXS: [u8; U128_MAX.const_txs_size()] = {
        let mut w = xdrs::txs::ConstWriter::<{ U128_MAX.const_txs_size() }>::new();
        U128_MAX.const_write_txs(&mut w);
        w.into_bytes()
    };

    const U128_ZERO_CXS: [u8; U128_ZERO.const_cxs_size()] = {
        let mut w = xdrs::cxs::ConstWriter::<{ U128_ZERO.const_cxs_size() }>::new();
        U128_ZERO.const_write_cxs(&mut w);
        w.into_bytes()
    };

    const U128_MAX_CXS: [u8; U128_MAX.const_cxs_size()] = {
        let mut w = xdrs::cxs::ConstWriter::<{ U128_MAX.const_cxs_size() }>::new();
        U128_MAX.const_write_cxs(&mut w);
        w.into_bytes()
    };

    #[test]
    fn i128_u128_boundary_values() {
        assert_eq!(&U128_ZERO_TXS, txs_runtime_encode(&U128_ZERO).as_slice());
        assert_eq!(&U128_MAX_TXS, txs_runtime_encode(&U128_MAX).as_slice());
        assert_eq!(&U128_ZERO_CXS, cxs_runtime_encode(&U128_ZERO).as_slice());
        assert_eq!(&U128_MAX_CXS, cxs_runtime_encode(&U128_MAX).as_slice());
    }
}

// -- &str const tests --

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct WithStr<'a> {
    label: &'a str,
    code: u32,
}

const WITH_STR: WithStr<'static> = WithStr {
    label: "hello",
    code: 42,
};

const WITH_STR_EMPTY: WithStr<'static> = WithStr { label: "", code: 0 };

const WITH_STR_TXS: [u8; WITH_STR.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ WITH_STR.const_txs_size() }>::new();
    WITH_STR.const_write_txs(&mut w);
    w.into_bytes()
};

const WITH_STR_CXS: [u8; WITH_STR.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WITH_STR.const_cxs_size() }>::new();
    WITH_STR.const_write_cxs(&mut w);
    w.into_bytes()
};

const WITH_STR_EMPTY_TXS: [u8; WITH_STR_EMPTY.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ WITH_STR_EMPTY.const_txs_size() }>::new();
    WITH_STR_EMPTY.const_write_txs(&mut w);
    w.into_bytes()
};

const WITH_STR_EMPTY_CXS: [u8; WITH_STR_EMPTY.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WITH_STR_EMPTY.const_cxs_size() }>::new();
    WITH_STR_EMPTY.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn str_ref_const_matches_runtime() {
    assert_eq!(&WITH_STR_TXS, txs_runtime_encode(&WITH_STR).as_slice());
    assert_eq!(&WITH_STR_CXS, cxs_runtime_encode(&WITH_STR).as_slice());
    assert_eq!(
        WITH_STR.const_txs_size(),
        txs_runtime_encode(&WITH_STR).len()
    );
    assert_eq!(
        WITH_STR.const_cxs_size(),
        cxs_runtime_encode(&WITH_STR).len()
    );
}

#[test]
fn str_ref_empty_const_matches_runtime() {
    assert_eq!(
        &WITH_STR_EMPTY_TXS,
        txs_runtime_encode(&WITH_STR_EMPTY).as_slice()
    );
    assert_eq!(
        &WITH_STR_EMPTY_CXS,
        cxs_runtime_encode(&WITH_STR_EMPTY).as_slice()
    );
    assert_eq!(
        WITH_STR_EMPTY.const_txs_size(),
        txs_runtime_encode(&WITH_STR_EMPTY).len()
    );
    assert_eq!(
        WITH_STR_EMPTY.const_cxs_size(),
        cxs_runtime_encode(&WITH_STR_EMPTY).len()
    );
}

// -- &[u8] const tests --

#[derive(Debug, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct WithByteSlice<'a> {
    data: &'a [u8],
    tag: u32,
}

const WITH_BYTES: WithByteSlice<'static> = WithByteSlice {
    data: &[0xDE, 0xAD, 0xBE, 0xEF],
    tag: 1,
};

const WITH_BYTES_EMPTY: WithByteSlice<'static> = WithByteSlice { data: &[], tag: 0 };

const WITH_BYTES_TXS: [u8; WITH_BYTES.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ WITH_BYTES.const_txs_size() }>::new();
    WITH_BYTES.const_write_txs(&mut w);
    w.into_bytes()
};

const WITH_BYTES_CXS: [u8; WITH_BYTES.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WITH_BYTES.const_cxs_size() }>::new();
    WITH_BYTES.const_write_cxs(&mut w);
    w.into_bytes()
};

const WITH_BYTES_EMPTY_TXS: [u8; WITH_BYTES_EMPTY.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ WITH_BYTES_EMPTY.const_txs_size() }>::new();
    WITH_BYTES_EMPTY.const_write_txs(&mut w);
    w.into_bytes()
};

const WITH_BYTES_EMPTY_CXS: [u8; WITH_BYTES_EMPTY.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WITH_BYTES_EMPTY.const_cxs_size() }>::new();
    WITH_BYTES_EMPTY.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn byte_slice_const_matches_runtime() {
    assert_eq!(&WITH_BYTES_TXS, txs_runtime_encode(&WITH_BYTES).as_slice());
    assert_eq!(&WITH_BYTES_CXS, cxs_runtime_encode(&WITH_BYTES).as_slice());
    assert_eq!(
        WITH_BYTES.const_txs_size(),
        txs_runtime_encode(&WITH_BYTES).len()
    );
    assert_eq!(
        WITH_BYTES.const_cxs_size(),
        cxs_runtime_encode(&WITH_BYTES).len()
    );
}

#[test]
fn byte_slice_empty_const_matches_runtime() {
    assert_eq!(
        &WITH_BYTES_EMPTY_TXS,
        txs_runtime_encode(&WITH_BYTES_EMPTY).as_slice()
    );
    assert_eq!(
        &WITH_BYTES_EMPTY_CXS,
        cxs_runtime_encode(&WITH_BYTES_EMPTY).as_slice()
    );
    assert_eq!(
        WITH_BYTES_EMPTY.const_txs_size(),
        txs_runtime_encode(&WITH_BYTES_EMPTY).len()
    );
    assert_eq!(
        WITH_BYTES_EMPTY.const_cxs_size(),
        cxs_runtime_encode(&WITH_BYTES_EMPTY).len()
    );
}

// -- None / Ok / Some / Err path tests --

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct OptionResultPaths {
    absent: Option<Leaf>,
    nested_absent: Option<Option<Leaf>>,
    success: Result<Leaf, Leaf>,
}

const NONE_AND_OK: OptionResultPaths = OptionResultPaths {
    absent: None,
    nested_absent: Some(None),
    success: Ok(LEAF_A),
};

const SOME_AND_ERR: OptionResultPaths = OptionResultPaths {
    absent: Some(LEAF_A),
    nested_absent: Some(Some(LEAF_A)),
    success: Err(LEAF_A),
};

const NONE_AND_OK_TXS: [u8; NONE_AND_OK.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ NONE_AND_OK.const_txs_size() }>::new();
    NONE_AND_OK.const_write_txs(&mut w);
    w.into_bytes()
};

const NONE_AND_OK_CXS: [u8; NONE_AND_OK.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ NONE_AND_OK.const_cxs_size() }>::new();
    NONE_AND_OK.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn none_and_ok_const_matches_runtime() {
    assert_eq!(
        &NONE_AND_OK_TXS,
        txs_runtime_encode(&NONE_AND_OK).as_slice()
    );
    assert_eq!(
        &NONE_AND_OK_CXS,
        cxs_runtime_encode(&NONE_AND_OK).as_slice()
    );
    assert_eq!(
        NONE_AND_OK.const_txs_size(),
        txs_runtime_encode(&NONE_AND_OK).len()
    );
    assert_eq!(
        NONE_AND_OK.const_cxs_size(),
        cxs_runtime_encode(&NONE_AND_OK).len()
    );
}

const SOME_AND_ERR_TXS: [u8; SOME_AND_ERR.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ SOME_AND_ERR.const_txs_size() }>::new();
    SOME_AND_ERR.const_write_txs(&mut w);
    w.into_bytes()
};

const SOME_AND_ERR_CXS: [u8; SOME_AND_ERR.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ SOME_AND_ERR.const_cxs_size() }>::new();
    SOME_AND_ERR.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn some_and_err_const_matches_runtime() {
    assert_eq!(
        &SOME_AND_ERR_TXS,
        txs_runtime_encode(&SOME_AND_ERR).as_slice()
    );
    assert_eq!(
        &SOME_AND_ERR_CXS,
        cxs_runtime_encode(&SOME_AND_ERR).as_slice()
    );
    assert_eq!(
        SOME_AND_ERR.const_txs_size(),
        txs_runtime_encode(&SOME_AND_ERR).len()
    );
    assert_eq!(
        SOME_AND_ERR.const_cxs_size(),
        cxs_runtime_encode(&SOME_AND_ERR).len()
    );
}

// -- Enum with tuple variants (unnamed fields) --

#[derive(Debug, Clone, Copy, PartialEq, Eq, xdrs::ConstWriteXDR, xdrs::WriteXDR)]
#[repr(i32)]
#[allow(dead_code)]
enum TupleShape {
    Circle(u32) = 0,
    Rect(u32, u32) = 1,
    Empty = 2,
}

const TCIRCLE: TupleShape = TupleShape::Circle(5);
const TRECT: TupleShape = TupleShape::Rect(10, 20);
const TEMPTY: TupleShape = TupleShape::Empty;

const TCIRCLE_TXS: [u8; TCIRCLE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TCIRCLE.const_txs_size() }>::new();
    TCIRCLE.const_write_txs(&mut w);
    w.into_bytes()
};

const TCIRCLE_CXS: [u8; TCIRCLE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TCIRCLE.const_cxs_size() }>::new();
    TCIRCLE.const_write_cxs(&mut w);
    w.into_bytes()
};

const TRECT_TXS: [u8; TRECT.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TRECT.const_txs_size() }>::new();
    TRECT.const_write_txs(&mut w);
    w.into_bytes()
};

const TRECT_CXS: [u8; TRECT.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TRECT.const_cxs_size() }>::new();
    TRECT.const_write_cxs(&mut w);
    w.into_bytes()
};

const TEMPTY_TXS: [u8; TEMPTY.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ TEMPTY.const_txs_size() }>::new();
    TEMPTY.const_write_txs(&mut w);
    w.into_bytes()
};

const TEMPTY_CXS: [u8; TEMPTY.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ TEMPTY.const_cxs_size() }>::new();
    TEMPTY.const_write_cxs(&mut w);
    w.into_bytes()
};

#[test]
fn enum_tuple_variants_const_matches_runtime() {
    assert_eq!(&TCIRCLE_TXS, txs_runtime_encode(&TCIRCLE).as_slice());
    assert_eq!(&TCIRCLE_CXS, cxs_runtime_encode(&TCIRCLE).as_slice());
    assert_eq!(&TRECT_TXS, txs_runtime_encode(&TRECT).as_slice());
    assert_eq!(&TRECT_CXS, cxs_runtime_encode(&TRECT).as_slice());
    assert_eq!(&TEMPTY_TXS, txs_runtime_encode(&TEMPTY).as_slice());
    assert_eq!(&TEMPTY_CXS, cxs_runtime_encode(&TEMPTY).as_slice());
}

#[test]
fn enum_tuple_variants_sizes_match() {
    assert_eq!(TCIRCLE.const_txs_size(), txs_runtime_encode(&TCIRCLE).len());
    assert_eq!(TCIRCLE.const_cxs_size(), cxs_runtime_encode(&TCIRCLE).len());
    assert_eq!(TRECT.const_txs_size(), txs_runtime_encode(&TRECT).len());
    assert_eq!(TRECT.const_cxs_size(), cxs_runtime_encode(&TRECT).len());
    assert_eq!(TEMPTY.const_txs_size(), txs_runtime_encode(&TEMPTY).len());
    assert_eq!(TEMPTY.const_cxs_size(), cxs_runtime_encode(&TEMPTY).len());
}

// -- u32 length-bound tests (issue #55) --
// Guarded to 64-bit: u32::MAX as usize + 1 overflows usize on 32-bit targets.
#[cfg(target_pointer_width = "64")]
mod overlong_length_bound {
    use super::*;

    #[derive(Clone, Copy)]
    struct Zst;

    impl Zst {
        pub const fn const_write_cxs<const N: usize>(&self, _: &mut xdrs::cxs::ConstWriter<N>) {}
        pub const fn const_write_txs<const N: usize>(&self, _: &mut xdrs::txs::ConstWriter<N>) {}
        pub const fn const_cxs_size(&self) -> usize {
            0
        }
        pub const fn const_txs_size(&self) -> usize {
            0
        }
    }

    impl xdrs::WriteXDR for Zst {
        fn write_xdr<F, W: xdrs::Writer<F>>(&self, _: &mut W) -> xdrs::WriteResult<()> {
            Ok(())
        }
    }

    #[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
    struct ZstSliceHolder<'a> {
        items: &'a [Zst],
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_slice_txs_size_rejects_overlong() {
        let items = vec![Zst; u32::MAX as usize + 1];
        let holder = ZstSliceHolder { items: &items };
        let _ = holder.const_txs_size();
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_slice_cxs_size_rejects_overlong() {
        let items = vec![Zst; u32::MAX as usize + 1];
        let holder = ZstSliceHolder { items: &items };
        let _ = holder.const_cxs_size();
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_slice_txs_write_rejects_overlong() {
        let items = vec![Zst; u32::MAX as usize + 1];
        let holder = ZstSliceHolder { items: &items };
        let mut w = xdrs::txs::ConstWriter::<4>::new();
        holder.const_write_txs(&mut w);
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_slice_cxs_write_rejects_overlong() {
        let items = vec![Zst; u32::MAX as usize + 1];
        let holder = ZstSliceHolder { items: &items };
        let mut w = xdrs::cxs::ConstWriter::<1>::new();
        holder.const_write_cxs(&mut w);
    }

    const OVERLONG: usize = u32::MAX as usize + 1;

    #[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
    struct ZstArrayHolder {
        items: [Zst; OVERLONG],
    }

    fn make_overlong_array() -> ZstArrayHolder {
        ZstArrayHolder {
            items: [Zst; OVERLONG],
        }
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_array_txs_size_rejects_overlong() {
        let holder = make_overlong_array();
        let _ = holder.const_txs_size();
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_array_cxs_size_rejects_overlong() {
        let holder = make_overlong_array();
        let _ = holder.const_cxs_size();
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_array_txs_write_rejects_overlong() {
        let holder = make_overlong_array();
        let mut w = xdrs::txs::ConstWriter::<0>::new();
        holder.const_write_txs(&mut w);
    }

    #[test]
    #[should_panic(expected = "u32")]
    fn const_array_cxs_write_rejects_overlong() {
        let holder = make_overlong_array();
        let mut w = xdrs::cxs::ConstWriter::<0>::new();
        holder.const_write_cxs(&mut w);
    }
}

// Positive test: normal-sized typed slice still encodes correctly through const path.
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct LeafSliceHolder<'a> {
    items: &'a [Leaf],
}

#[test]
fn const_typed_slice_matches_runtime() {
    let items = [LEAF_A, LEAF_B];
    let holder = LeafSliceHolder { items: &items };

    assert_eq!(holder.const_cxs_size(), cxs_runtime_encode(&holder).len());
    assert_eq!(holder.const_txs_size(), txs_runtime_encode(&holder).len());
}

// -- PhantomData tests (issue #88) --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct WithPhantom {
    tag: u32,
    marker: core::marker::PhantomData<u32>,
}

const WITH_PHANTOM: WithPhantom = WithPhantom {
    tag: 7,
    marker: core::marker::PhantomData,
};

const WITH_PHANTOM_CXS: [u8; WITH_PHANTOM.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WITH_PHANTOM.const_cxs_size() }>::new();
    WITH_PHANTOM.const_write_cxs(&mut w);
    w.into_bytes()
};

const WITH_PHANTOM_TXS: [u8; WITH_PHANTOM.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ WITH_PHANTOM.const_txs_size() }>::new();
    WITH_PHANTOM.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn phantom_data_const_matches_runtime() {
    assert_eq!(
        &WITH_PHANTOM_CXS,
        cxs_runtime_encode(&WITH_PHANTOM).as_slice()
    );
    assert_eq!(
        &WITH_PHANTOM_TXS,
        txs_runtime_encode(&WITH_PHANTOM).as_slice()
    );
    assert_eq!(
        WITH_PHANTOM.const_cxs_size(),
        cxs_runtime_encode(&WITH_PHANTOM).len()
    );
    assert_eq!(
        WITH_PHANTOM.const_txs_size(),
        txs_runtime_encode(&WITH_PHANTOM).len()
    );
}

// PhantomData newtype wrapper
#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct PhantomNewtype(core::marker::PhantomData<u32>);

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct WithPhantomNewtype {
    tag: u32,
    marker: PhantomNewtype,
}

const WITH_PHANTOM_NT: WithPhantomNewtype = WithPhantomNewtype {
    tag: 7,
    marker: PhantomNewtype(core::marker::PhantomData),
};

const WITH_PHANTOM_NT_CXS: [u8; WITH_PHANTOM_NT.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WITH_PHANTOM_NT.const_cxs_size() }>::new();
    WITH_PHANTOM_NT.const_write_cxs(&mut w);
    w.into_bytes()
};

const WITH_PHANTOM_NT_TXS: [u8; WITH_PHANTOM_NT.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ WITH_PHANTOM_NT.const_txs_size() }>::new();
    WITH_PHANTOM_NT.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn phantom_newtype_const_matches_runtime() {
    assert_eq!(
        &WITH_PHANTOM_NT_CXS,
        cxs_runtime_encode(&WITH_PHANTOM_NT).as_slice()
    );
    assert_eq!(
        &WITH_PHANTOM_NT_TXS,
        txs_runtime_encode(&WITH_PHANTOM_NT).as_slice()
    );
    assert_eq!(
        WITH_PHANTOM_NT.const_cxs_size(),
        cxs_runtime_encode(&WITH_PHANTOM_NT).len()
    );
    assert_eq!(
        WITH_PHANTOM_NT.const_txs_size(),
        txs_runtime_encode(&WITH_PHANTOM_NT).len()
    );
    // Encoding should match the non-newtype version (PhantomData contributes zero bytes)
    assert_eq!(&WITH_PHANTOM_NT_CXS, &WITH_PHANTOM_CXS);
    assert_eq!(&WITH_PHANTOM_NT_TXS, &WITH_PHANTOM_TXS);
}

// -- Infallible tests (issue #88) --

#[derive(xdrs::ConstWriteXDR, xdrs::WriteXDR)]
struct WithInfallible {
    ok: Result<u32, core::convert::Infallible>,
    never: Option<core::convert::Infallible>,
}

const WITH_INFALLIBLE: WithInfallible = WithInfallible {
    ok: Ok(7),
    never: None,
};

const WITH_INFALLIBLE_CXS: [u8; WITH_INFALLIBLE.const_cxs_size()] = {
    let mut w = xdrs::cxs::ConstWriter::<{ WITH_INFALLIBLE.const_cxs_size() }>::new();
    WITH_INFALLIBLE.const_write_cxs(&mut w);
    w.into_bytes()
};

const WITH_INFALLIBLE_TXS: [u8; WITH_INFALLIBLE.const_txs_size()] = {
    let mut w = xdrs::txs::ConstWriter::<{ WITH_INFALLIBLE.const_txs_size() }>::new();
    WITH_INFALLIBLE.const_write_txs(&mut w);
    w.into_bytes()
};

#[test]
fn infallible_const_matches_runtime() {
    assert_eq!(
        &WITH_INFALLIBLE_CXS,
        cxs_runtime_encode(&WITH_INFALLIBLE).as_slice()
    );
    assert_eq!(
        &WITH_INFALLIBLE_TXS,
        txs_runtime_encode(&WITH_INFALLIBLE).as_slice()
    );
    assert_eq!(
        WITH_INFALLIBLE.const_cxs_size(),
        cxs_runtime_encode(&WITH_INFALLIBLE).len()
    );
    assert_eq!(
        WITH_INFALLIBLE.const_txs_size(),
        txs_runtime_encode(&WITH_INFALLIBLE).len()
    );
}
