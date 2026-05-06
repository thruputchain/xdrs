use xdrs::*;

#[derive(XDR)]
#[repr(i32)]
enum Status {
    Ready = 10,
    Failed = 11,
}

#[derive(ReadXDR, WriteXDR, XDR)]
#[xdr(discrim_type = "Status")]
#[repr(i32)]
enum Message {
    Ready { value: u32 },
    Failed { code: u32 },
}

fn main() {}
