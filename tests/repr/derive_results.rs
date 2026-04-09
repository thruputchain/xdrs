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
