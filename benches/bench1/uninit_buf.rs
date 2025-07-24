use core::mem::MaybeUninit as MU;
use xdrs::mu;
use xdrs::serialize::writer::Result as WriteResult;

pub struct UninitBuf<const N: usize> {
    buf: [MU<u8>; N],
    valid_len: usize,
}

impl<const N: usize> UninitBuf<N> {
    /// Create an uninitialized buffer with byte-length `N`
    pub const fn new() -> Self {
        Self {
            buf: [MU::uninit(); N],
            valid_len: 0,
        }
    }

    /// Initialize some prefix of the buffer, reporting
    /// how many bytes were initialized
    pub fn init<F>(&mut self, f: F) -> WriteResult<()>
    where
        F: Fn(&mut [MU<u8>]) -> WriteResult<usize>,
    {
        self.valid_len = f(&mut self.buf)?;
        Ok(())
    }

    /// Get a reference to the now-allegedly-initialized portion of the buffer
    ///
    /// # Safety
    ///
    /// The function most-recently passed to init, if any,
    /// must have initialized the buffer prefix it claimed to.
    pub unsafe fn assume_init(&self) -> &[u8] {
        unsafe { mu::assume_init_ref(&self.buf[..self.valid_len]) }
    }
}

impl<const N: usize> Default for UninitBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}
