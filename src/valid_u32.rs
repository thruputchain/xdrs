/// A hack to clamp const generic usize at compile-time

pub struct ValidU32<const N: usize>;

impl<const N: usize> ValidU32<N> {
    pub const OK: () = assert!(N <= u32::MAX as usize, "N must fit in a u32");
}
