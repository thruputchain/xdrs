use core::marker::PhantomData;

use xdr_common::BytesM;
use xdr_repr::*;
use xdrs::*;

#[derive(XDR)]
pub struct Wrapper {
    pub choice: Result<bool, u32>,
}

check_format! { derive_struct_with_result_field
    Wrapper::def()
    =>
        "struct Wrapper {"
        "\tunion switch (int which)"
        "\t{"
        "\tcase 0:"
        "\t\tbool ok;"
        "\tcase 1:"
        "\t\tunsigned int err;"
        "\t} choice;"
        "};"
}

#[derive(XDR)]
pub struct ResultBytesUnit {
    pub payload: Result<BytesM<4>, ()>,
}

check_format! { result_bytesm_unit_err
    ResultBytesUnit::def()
    =>
        "struct ResultBytesUnit {"
        "\tunion switch (int which)"
        "\t{"
        "\tcase 0:"
        "\t\topaque ok<4>;"
        "\tcase 1:"
        "\t\tvoid;"
        "\t} payload;"
        "};"
}

#[derive(XDR)]
pub struct ResultBytesPhantom {
    pub payload: Result<BytesM<4>, PhantomData<u32>>,
}

check_format! { result_bytesm_phantomdata_err
    ResultBytesPhantom::def()
    =>
        "struct ResultBytesPhantom {"
        "\tunion switch (int which)"
        "\t{"
        "\tcase 0:"
        "\t\topaque ok<4>;"
        "\tcase 1:"
        "\t\tvoid;"
        "\t} payload;"
        "};"
}
