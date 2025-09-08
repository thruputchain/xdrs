use crate::mu;
use crate::serialize::reader::*;
use crate::valid_u32::*;
use core::mem::MaybeUninit as MU;

/// Types that can be constructed from XDR, possibly sharing data with the
/// input. A state value of the associated type will be available to
/// implementations of `read_xdr`, via the reader's `AsRef<State>`
/// implementation.
pub trait ReadXDR<'r>: Sized {
    type State;

    fn read_xdr<R>(r: &mut R) -> Result<Self>
        where
            R: Reader<'r>;
}

pub fn read_xdr<'r, R, S, U, T>(r: &mut R) -> Result<T>
where
    T: ReadXDR<'r, State = S>,
    R: Reader<'r, State = U>,
    for<'a> S: From<&'a U>
{
    T::read_xdr(r)
}

macro_rules! construct_int_impls {
    { $( $read_method:ident for $int_ty:ty ),* $(,)? } => { $(

        impl<'r> ReadXDR<'r> for $int_ty {
            type State = ();

            fn read_xdr<R, S>(r: &mut R) -> Result<$int_ty>
            where
                R: Reader<'r, State = S>,
                for<'a> Self::State: From<&'a S>,
            {
                r.$read_method()
            }
        }

    )* };
}

construct_int_impls! {
    read_i64 for i64,
    read_i32 for i32,
    read_i16 for i16,

    read_u64 for u64,
    read_u32 for u32,
    read_u16 for u16,

    read_bool for bool,
}

macro_rules! impl_read_xdr {
    { $(<const $n:ident: $nt:ty>)? $r:ident -> $ty:ty $body:block } => {

        impl<'r, $(const $n: $nt)?> ReadXDR<'r> for $ty {
            type State = ();

            fn read_xdr<R, S>($r: &mut R) -> Result<$ty>
            where
                R: Reader<'r, State = S>,
                for<'a> Self::State: From<&'a S>,
            $body
        }

    };
    { <$l:lifetime, const $n:ident: $nt:ty>($r:ident) -> $ty:ty $body:block } => {

        impl<$l, const $n: $nt> ReadXDR<$l> for $ty {
            type State = ();

            fn read_xdr<R, S>($r: &mut R) -> Result<$ty>
            where
                R: Reader<$l, State = S>,
                for<'a> Self::State: From<&'a S>,
            $body
        }

    };
}


#[cfg(all(feature = "extras", not(feature = "int128_xdr")))]
const _: () = {
    impl<'r, S> ReadXDR<'r> for i128 {
        type State = ();

        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };
            Ok(Self::from_le_bytes(bytes))
        }
    }

    impl<'r, S> ReadXDR<'r> for u128 {
        type State = ();

        fn read_xdr<R: Reader<'r>>(r: &mut R) -> Result<Self> {
            let mut uninit = [MU::<u8>::zeroed(); 16];

            let _ = r.read_opaque_variable_into(&mut uninit, 16)?;

            // SAFETY: the pre-zeroed bytes are valid u8's
            let bytes = unsafe { mu::array_assume_init(uninit) };
            Ok(Self::from_le_bytes(bytes))
        }
    }
};

// Another way to serialize 128-bit ints
#[cfg(all(feature = "extras", feature = "int128_xdr"))]
const _: () = {

    impl_read_xdr!(r -> i128 {
        let highbits: i64 = read_xdr(r)?;
        let lowbits: u64 = read_xdr(r)?;
        let i: i128 = ((highbits as i128) << 64) | (lowbits as i128);

        Ok(i)
    });

    impl_read_xdr!(r -> u128 {
        let highbits: u64 = read_xdr(r)?;
        let lowbits: u64 = read_xdr(r)?;
        let u: u128 = ((highbits as u128) << 64) | (lowbits as u128);

        Ok(u)
    });
};

impl<'r, T, const N: usize> ReadXDR<'r> for [T; N]
where
    T: ReadXDR<'r>,
{
    type State = <T as ReadXDR<'r>>::State;

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<T>; N] = [const { MU::uninit() }; N];

        r.seq(Some(N))?;

        // TODO: catch errors to avoid memory leaks
        for elt in arr.iter_mut() {
            elt.write(read_xdr(r)?);
            r.next()?;
        }

        r.end()?;

        // SAFETY: all elements have been initialized
        let arr = unsafe { mu::array_assume_init(arr) };
        Ok(arr)
    }
}

impl<'r, const N: usize> ReadXDR<'r> for [u8; N] {
    type State = ();

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        let () = ValidU32::<N>::OK;

        let mut arr: [MU<u8>; N] = [MU::uninit(); N];

        r.read_opaque_fixed_into(&mut arr, N as u32)?;

        // SAFETY: We rely on `read_opaque_fixed_into` to init all elements
        let arr = unsafe { mu::array_assume_init(arr) };

        Ok(arr)
    }
}

/*
impl_read_xdr!(<'r, const N: usize>(r) -> &'r [u8; N] {
    let () = ValidU32::<N>::OK;
    let len = N as u32;

    // PANIC: We rely on `Reader::read_opaque_fixed` to return a slice of
    // the requested size
    let bytes: &[u8; N] = r.read_opaque_fixed(len)?.try_into().unwrap();
    Ok(bytes)
});
*/

impl<'r, const N: usize> ReadXDR<'r> for &'r [u8; N] {
    type State = ();

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        let () = ValidU32::<N>::OK;
        let len = N as u32;

        // PANIC: We rely on `Reader::read_opaque_fixed` to return a slice of
        // the requested size
        let bytes: &[u8; N] = r.read_opaque_fixed(len)?.try_into().unwrap();
        Ok(bytes)
    }
}

impl<'r> ReadXDR<'r> for &'r [u8] {
    type State = ();

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        r.read_opaque_variable(u32::MAX)
    }
}

#[cfg(feature = "alloc")]
impl<'r> ReadXDR<'r> for alloc::string::String {
    type State = ();

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        let bytes = r.read_opaque_variable(u32::MAX)?;
        let v = alloc::vec::Vec::from(bytes);
        let s = alloc::string::String::from_utf8(v).map_err(|_| Error::BadUTF8)?;
        Ok(s)
    }
}

impl_read_xdr!(_r -> () {
    Ok(())
});

macro_rules! read_tuple {
    { $n:expr => $( $t:ident ),+ } => {

        impl<'r, U, $($t),+> ReadXDR<'r> for ($($t),+)
        where
            $( $t: ReadXDR<'r, State = U>, )+
        {
            type State = U;

            fn read_xdr<R, S>(r: &mut R) -> Result<Self>
            where
                R: Reader<'r, State = S>,
                for<'a> Self::State: From<&'a S>,
            {
                r.seq(Some($n))?;
                let v = ( $({ let v: $t = read_xdr(r)?; r.next()?; v }),+ );
                r.end()?;
                Ok(v)
            }
        }

    };
}

read_tuple!(2 => T1, T2);
read_tuple!(3 => T1, T2, T3);
read_tuple!(4 => T1, T2, T3, T4);
read_tuple!(5 => T1, T2, T3, T4, T5);
read_tuple!(6 => T1, T2, T3, T4, T5, T6);

impl<'r, T> ReadXDR<'r> for Option<T>
where
    T: ReadXDR<'r>,
{
    type State = <T as ReadXDR<'r>>::State;

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        let is_present = r.read_bool()?;
        if is_present {
            Ok(Some(read_xdr(r)?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(all(feature = "xdr_common", feature = "alloc"))]
impl<'r, T> ReadXDR<'r> for xdr_common::Box<T>
where
    T: ReadXDR<'r>,
{
    type State = <T as ReadXDR<'r>>::State;

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        Ok(Self::new(read_xdr(r)?))
    }
}

impl<'r, U, T, E> ReadXDR<'r> for core::result::Result<T, E>
where
    T: ReadXDR<'r, State = U>,
    E: ReadXDR<'r, State = U>,
{
    type State = U;

    fn read_xdr<R, S>(r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        let d = r.read_i32()?;

        match d {
            0 => Ok(Result::Ok(read_xdr(r)?)),
            1 => Ok(Result::Err(read_xdr(r)?)),
            _ => Err(Error::BadDiscriminator(d)),
        }
    }
}

impl<'r, T> ReadXDR<'r> for core::marker::PhantomData<T> {
    type State = ();

    fn read_xdr<R, S>(_r: &mut R) -> Result<Self>
    where
        R: Reader<'r, State = S>,
        for<'a> Self::State: From<&'a S>,
    {
        Ok(core::marker::PhantomData)
    }
}

#[cfg(feature = "alloc")]
const _: () = {
    use alloc::vec::Vec;

    impl_read_xdr!(r -> Vec<u8> {
        let bytes: &[u8] = r.read_opaque_variable(u32::MAX)?;
        let vec = Vec::from(bytes); // copy the slice
        Ok(vec)
    });

    impl<'r, T> ReadXDR<'r> for Vec<T>
    where
        T: ReadXDR<'r>,
    {
        type State = <T as ReadXDR<'r>>::State;

        fn read_xdr<R, S>(r: &mut R) -> Result<Self>
        where
            R: Reader<'r, State = S>,
            for<'a> Self::State: From<&'a S>,
        {
            let len = r.read_u32()? as usize;
            r.seq(Some(len))?;

            let mut vec: Vec<T> = Vec::with_capacity(len);
            let slice = vec.spare_capacity_mut();

            // TODO: catch errors to avoid memory leaks
            for elt in slice.iter_mut() {
                elt.write(read_xdr(r)?);
                r.next()?;
            }

            r.end()?;

            // SAFETY: all elts up to `len` have been initialized
            unsafe { vec.set_len(len) }

            Ok(vec)
        }
    }
};

#[cfg(all(feature = "xdr_common", feature = "alloc"))]
const _: () = {
    use xdr_common::*;

    use crate::serialize::reader::{Error, Reader, Result};
    use crate::{ReadXDR, read_xdr};

    use alloc::vec::Vec; // can use noalloc machinery in common_types to avoid this

    impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        type State = ();

        fn read_xdr<R, S>(r: &mut R) -> Result<Self>
        where
            R: Reader<'r, State = S>,
            for<'a> Self::State: From<&'a S>,
        {
            let vec: Vec<u8> = read_xdr(r)?;
            if vec.len() > MAX as usize {
                return Err(Error::MaxLengthExceeded);
            }
            Ok(Self(vec))
        }
    }

    impl<'r, T, const MAX: u32> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r>,
    {
        type State = <T as ReadXDR<'r>>::State;

        fn read_xdr<R, S>(r: &mut R) -> Result<Self>
        where
            R: Reader<'r, State = S>,
            for<'a> Self::State: From<&'a S>,
        {
            let vec: Vec<T> = read_xdr(r)?;
            if vec.len() > MAX as usize {
                return Err(Error::MaxLengthExceeded);
            }
            Ok(Self(vec))
        }
    }
};

#[cfg(all(feature = "xdr_common", not(feature = "alloc")))]
const _: () = {
    use crate::ReadXDR;
    use crate::serialize::reader::{Reader, Result};
    use xdr_common::*;

    impl<'r, const MAX: u32> ReadXDR<'r> for BytesM<MAX> {
        type State = ();

        fn read_xdr<R, S>(r: &mut R) -> Result<Self>
        where
            R: Reader<'r, State = S>,
            for<'a> Self::State: From<&'a S>,
        {
            unimplemented!()
        }
    }

    impl<'r, const MAX: u32, T> ReadXDR<'r> for VecM<T, MAX>
    where
        T: ReadXDR<'r>,
    {
        type State = <T as ReadXDR<'r>>::State;

        fn read_xdr<R, S>(r: &mut R) -> Result<Self>
        where
            R: Reader<'r, State = S>,
            for<'a> Self::State: From<&'a S>,
        {
            unimplemented!()
        }
    }
};
