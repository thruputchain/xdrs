// Newtypes for various kinds of byte sequences,
// allowing specialized Construct/Deconstuct implementations

use core::ops::Deref;

#[derive(Debug, PartialEq, Eq)]
pub struct Bytes<'a>(&'a [u8]);

impl<'a> From<&'a [u8]> for Bytes<'a> {
    fn from(slice: &'a [u8]) -> Self {
        Self(slice)
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for Bytes<'a> {
    fn from(array: &'a [u8; N]) -> Self {
        Self(&array[..])
    }
}

impl<'a> AsRef<[u8]> for Bytes<'a> {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ByteArray<'a, const N: usize>(&'a [u8; N]);

impl<'a, const N: usize> From<&'a [u8; N]> for ByteArray<'a, N> {
    fn from(array: &'a [u8; N]) -> Self {
        Self(array)
    }
}

impl<'a, const N: usize> AsRef<[u8; N]> for ByteArray<'a, N> {
    fn as_ref(&self) -> &[u8; N] {
        self.0
    }
}

impl<'a, const N: usize> AsRef<[u8]> for ByteArray<'a, N> {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedByteArray<const N: usize>([u8; N]);

impl<const N: usize> From<[u8; N]> for OwnedByteArray<N> {
    fn from(array: [u8; N]) -> Self {
        Self(array)
    }
}

impl<const N: usize> Deref for OwnedByteArray<N> {
    type Target = [u8; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
