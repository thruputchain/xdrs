use xdr_repr::*;
use xdrs::*;

#[derive(XDR)]
pub struct Measurement {
    pub temperature: f32,
    pub ratio: f64,
}

check_format! { derive_float_struct
    Measurement::def()
    =>
        "struct Measurement {"
        "	float temperature;"
        "	double ratio;"
        "};"
}
