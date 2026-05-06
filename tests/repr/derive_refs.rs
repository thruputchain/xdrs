use xdr_repr::*;
use xdrs::*;

#[derive(XDR)]
pub struct Leaf {
    pub code: u32,
}

pub mod shared_ref_named {
    use super::*;

    #[derive(XDR)]
    pub struct BorrowedLeaf<'a> {
        pub inner: &'a Leaf,
    }

    check_format! { shared_ref_named_type
        BorrowedLeaf::def()
        =>
            "struct BorrowedLeaf {"
            "	Leaf inner;"
            "};"
    }
}

pub mod mut_ref_named {
    use super::*;

    #[derive(XDR)]
    pub struct MutLeaf<'a> {
        pub inner: &'a mut Leaf,
    }

    check_format! { mut_ref_named_type
        MutLeaf::def()
        =>
            "struct MutLeaf {"
            "	Leaf inner;"
            "};"
    }
}

pub mod shared_ref_primitives {
    use super::*;

    #[derive(XDR)]
    pub struct Prims<'a> {
        pub a: &'a u32,
        pub b: &'a i32,
        pub c: &'a u64,
        pub d: &'a i64,
        pub e: &'a bool,
        pub f: &'a f32,
        pub g: &'a f64,
    }

    check_format! { shared_ref_primitives
        Prims::def()
        =>
            "struct Prims {"
            "	unsigned int a;"
            "	int b;"
            "	unsigned hyper c;"
            "	hyper d;"
            "	bool e;"
            "	float f;"
            "	double g;"
            "};"
    }
}

pub mod mut_ref_primitives {
    use super::*;

    #[derive(XDR)]
    pub struct MutPrims<'a> {
        pub a: &'a mut u32,
        pub b: &'a mut i32,
        pub c: &'a mut bool,
    }

    check_format! { mut_ref_primitives
        MutPrims::def()
        =>
            "struct MutPrims {"
            "	unsigned int a;"
            "	int b;"
            "	bool c;"
            "};"
    }
}

pub mod shared_ref_wrappers {
    use super::*;

    #[derive(XDR)]
    pub struct SharedWrappers<'a> {
        pub blob: &'a BytesM<8>,
        pub text: &'a StringM<8>,
        pub nums: &'a VecM<u32, 4>,
    }

    check_format! { shared_ref_wrappers
        SharedWrappers::def()
        =>
            "struct SharedWrappers {"
            "	opaque blob<8>;"
            "	string text<8>;"
            "	unsigned int nums<4>;"
            "};"
    }
}

pub mod mut_ref_wrappers {
    use super::*;

    #[derive(XDR)]
    pub struct MutWrappers<'a> {
        pub blob: &'a mut BytesM<8>,
        pub text: &'a mut StringM<8>,
        pub nums: &'a mut VecM<u32, 4>,
    }

    check_format! { mut_ref_wrappers
        MutWrappers::def()
        =>
            "struct MutWrappers {"
            "	opaque blob<8>;"
            "	string text<8>;"
            "	unsigned int nums<4>;"
            "};"
    }
}

pub mod shared_ref_alloc {
    use super::*;

    #[derive(XDR)]
    pub struct SharedAlloc<'a> {
        pub bytes: &'a Vec<u8>,
        pub items: &'a Vec<u32>,
        pub label: &'a String,
    }

    check_format! { shared_ref_alloc
        SharedAlloc::def()
        =>
            "struct SharedAlloc {"
            "	opaque bytes<>;"
            "	unsigned int items<>;"
            "	string label<>;"
            "};"
    }
}

pub mod mut_ref_alloc {
    use super::*;

    #[derive(XDR)]
    pub struct MutAlloc<'a> {
        pub bytes: &'a mut Vec<u8>,
        pub items: &'a mut Vec<u32>,
        pub label: &'a mut String,
    }

    check_format! { mut_ref_alloc
        MutAlloc::def()
        =>
            "struct MutAlloc {"
            "	opaque bytes<>;"
            "	unsigned int items<>;"
            "	string label<>;"
            "};"
    }
}

pub mod ref_slices {
    use super::*;

    #[derive(XDR)]
    pub struct SliceRefs<'a> {
        pub data: &'a [u32],
        pub name: &'a str,
    }

    check_format! { shared_ref_slices
        SliceRefs::def()
        =>
            "struct SliceRefs {"
            "	unsigned int data<>;"
            "	string name<>;"
            "};"
    }
}

pub mod mut_ref_slices {
    use super::*;

    #[derive(XDR)]
    pub struct MutSliceRefs<'a> {
        pub data: &'a mut [u32],
        pub bytes: &'a mut [u8],
        pub name: &'a mut str,
    }

    check_format! { mut_ref_slices
        MutSliceRefs::def()
        =>
            "struct MutSliceRefs {"
            "	unsigned int data<>;"
            "	opaque bytes<>;"
            "	string name<>;"
            "};"
    }
}
