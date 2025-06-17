#![allow(dead_code)]

// Print 6-bit one's complement signed ints

const I6_MAX: i8 = 31;
const I6_MIN: i8 = -31;

#[derive(Copy, Clone)]
struct I6 {
    nu: u8,
    neg: bool,
}

impl I6 {
    pub fn new(n: i8) -> Self {
        let mut neg = false;
        if n < 0 {
            neg = true;
        }
        let nu = n.unsigned_abs();

        if neg {
            if nu > -I6_MIN as u8 {
                panic!("too small for i6: {}", n)
            }
        } else {
            if nu > I6_MAX as u8 {
                panic!("too large for i6: {}", n)
            }
        }

        I6 { nu, neg }
    }

    pub fn as_i8(self) -> i8 {
        self.nu as i8 * if self.neg { -1 } else { 1 }
    }

    pub fn as_byte(self) -> u8 {
        if self.neg {
            0x20 | ((!self.nu) & 0x1f)
        } else {
            self.nu
        }
    }
}

#[test]
fn show_i6() {
    show_range(I6_MIN, I6_MIN+3);
    show_range(-3, 3);
    show_range(I6_MAX-3, I6_MAX);
}

fn show_range(min: i8, max: i8) {
    println!("");
    for m in min..=max {
        let n = I6::new(m);

        let byte = n.as_byte();
        let mut bin = format!("{:08b}", byte);
        bin.insert_str(2, "__");

        let n_i8 = n.as_i8();

        // show the number as decimal, then as 1-byte-encoding in hex and binary
        println!("I6 {n_i8:3} {byte:02x} {bin}");
    }
}

#[test]
fn show_i32() {
    println!("i32::MAX {} {}", i32::MAX, hex::encode(&i32::MAX.to_le_bytes()[..]));
    println!("i32::MIN {} {}", i32::MIN, hex::encode(&i32::MIN.to_le_bytes()[..]));
    println!("i32  32 {} {}", 32i32, hex::encode(&(32i32.to_le_bytes()[..])));
    println!("i32 -32 {} {}", -32i32, hex::encode(&((-32i32).to_le_bytes()[..])));
}
