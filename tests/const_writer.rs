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
