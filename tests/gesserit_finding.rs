use core::convert::Infallible;
use std::cell::Cell;
use std::panic;

#[cfg(feature = "alloc")]
use xdrs::VecM;
use xdrs::serialize::reader::{ReadError, Reader};
use xdrs::{ReadXDR, cxs, txs};

thread_local! {
    static ACTIVE_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug)]
struct LeakDetector {
    _id: u32,
    _payload: Vec<u8>,
}

impl LeakDetector {
    fn new(id: u32) -> Self {
        ACTIVE_COUNT.with(|count| count.set(count.get() + 1));

        Self {
            _id: id,
            _payload: vec![0xa5; 4096],
        }
    }
}

impl Drop for LeakDetector {
    fn drop(&mut self) {
        ACTIVE_COUNT.with(|count| count.set(count.get() - 1));
    }
}

impl<'r> ReadXDR<'r> for LeakDetector {
    fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self, ReadError> {
        Ok(Self::new(r.read_u32()?))
    }
}

fn reset_active_count() {
    ACTIVE_COUNT.with(|count| count.set(0));
}

fn active_count() -> usize {
    ACTIVE_COUNT.with(Cell::get)
}

fn assert_no_leak_on_unwind<F>(decode: F)
where
    F: FnOnce() + panic::UnwindSafe,
{
    reset_active_count();

    let result = panic::catch_unwind(decode);

    assert!(
        result.is_err(),
        "crafted bytes should unwind before returning a decode result"
    );
    assert_eq!(
        active_count(),
        0,
        "an initialized element was not dropped while unwinding"
    );
}

#[test]
fn poc_array_decode_unwind_drops_initialized_elements() {
    type Elem = Result<LeakDetector, Infallible>;

    #[rustfmt::skip]
    let txs_bytes: &[u8] = &[
        0x00, 0x00, 0x00, 0x00, // element 0: Result::Ok
        0x00, 0x00, 0x00, 0x05, // element 0: LeakDetector id = 5
        0x00, 0x00, 0x00, 0x01, // element 1: Result::Err(Infallible)
    ];

    assert_no_leak_on_unwind(|| {
        let _: Result<[Elem; 2], _> = txs::decode(txs_bytes);
    });

    #[rustfmt::skip]
    let cxs_bytes: &[u8] = &[
        0x00, // element 0: Result::Ok
        0x05, // element 0: LeakDetector id = 5
        0x01, // element 1: Result::Err(Infallible)
    ];

    assert_no_leak_on_unwind(|| {
        let _: Result<[Elem; 2], _> = cxs::decode(cxs_bytes);
    });
}

#[cfg(feature = "repr")]
#[test]
fn poc_negative_length_bounds_emit_invalid_schema() {
    use xdr_repr::{Declaration, TypeDef, TypeSpec, ident, signed};

    let cases: Vec<(Declaration, &str)> = vec![
        (
            Declaration::FixedOpaque {
                name: ident("data"),
                size: signed(-1),
            },
            "fixed opaque IDL",
        ),
        (
            Declaration::VariableOpaque {
                name: ident("blob"),
                max: signed(-1),
            },
            "variable opaque IDL",
        ),
        (
            Declaration::String {
                name: ident("name"),
                max: signed(-1),
            },
            "string IDL",
        ),
        (
            Declaration::FixedArray {
                name: ident("fixed_items"),
                elt_ty: TypeSpec::Unsigned,
                len: signed(-1),
            },
            "fixed array IDL",
        ),
        (
            Declaration::VariableArray {
                name: ident("variable_items"),
                elt_ty: TypeSpec::Int,
                max: signed(-1),
            },
            "variable array IDL",
        ),
    ];

    for (decl, label) in &cases {
        let decl_clone = decl.clone();
        let result = panic::catch_unwind(move || TypeDef::Def(decl_clone).to_string());
        assert!(
            result.is_err(),
            "{label}: should reject negative bound in IDL formatting"
        );
    }

    for (decl, label) in &cases {
        let decl_clone = decl.clone();
        let result = panic::catch_unwind(move || decl_clone.get_type_json());
        assert!(
            result.is_err(),
            "{label}: should reject negative bound in JSON output"
        );
    }
}

#[cfg(feature = "alloc")]
#[test]
fn poc_vecm_decode_unwind_drops_initialized_elements() {
    type Elem = Result<LeakDetector, Infallible>;
    type Values = VecM<Elem, 2>;

    #[rustfmt::skip]
    let txs_bytes: &[u8] = &[
        0x00, 0x00, 0x00, 0x02, // VecM length = 2
        0x00, 0x00, 0x00, 0x00, // element 0: Result::Ok
        0x00, 0x00, 0x00, 0x05, // element 0: LeakDetector id = 5
        0x00, 0x00, 0x00, 0x01, // element 1: Result::Err(Infallible)
    ];

    assert_no_leak_on_unwind(|| {
        let _: Result<Values, _> = txs::decode(txs_bytes);
    });

    #[rustfmt::skip]
    let cxs_bytes: &[u8] = &[
        0x02, // VecM length = 2
        0x00, // element 0: Result::Ok
        0x05, // element 0: LeakDetector id = 5
        0x01, // element 1: Result::Err(Infallible)
    ];

    assert_no_leak_on_unwind(|| {
        let _: Result<Values, _> = cxs::decode(cxs_bytes);
    });
}
