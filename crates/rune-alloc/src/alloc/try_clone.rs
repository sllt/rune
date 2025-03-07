use crate::alloc::Error;

/// Fallible `Clone` trait.
pub trait TryClone: Sized {
    /// Try to clone the current value, raising an allocation error if it's unsuccessful.
    fn try_clone(&self) -> Result<Self, Error>;

    /// Performs copy-assignment from `source`.
    ///
    /// `a.try_clone_from(&b)` is equivalent to `a = b.clone()` in
    /// functionality, but can be overridden to reuse the resources of `a` to
    /// avoid unnecessary allocations.
    #[inline]
    fn try_clone_from(&mut self, source: &Self) -> Result<(), Error> {
        *self = source.try_clone()?;
        Ok(())
    }
}

/// Marker trait for types which are `Copy`.
#[cfg_attr(rune_nightly, rustc_specialization_trait)]
pub trait TryCopy: TryClone {}

impl<T: ?Sized> TryClone for &T {
    #[inline]
    fn try_clone(&self) -> Result<Self, Error> {
        Ok(*self)
    }
}

macro_rules! impl_tuple {
    ($count:expr $(, $ty:ident $var:ident $num:expr)*) => {
        impl<$($ty,)*> TryClone for ($($ty,)*) where $($ty: TryClone,)* {
            #[inline]
            fn try_clone(&self) -> Result<Self, Error> {
                let ($($var,)*) = self;
                Ok(($($var.try_clone()?,)*))
            }
        }
    }
}

repeat_macro!(impl_tuple);

macro_rules! impl_copy {
    ($ty:ty) => {
        impl TryClone for $ty {
            #[inline]
            fn try_clone(&self) -> Result<Self, Error> {
                Ok(*self)
            }
        }

        impl TryCopy for $ty {}
    };
}

impl_copy!(usize);
impl_copy!(isize);
impl_copy!(u8);
impl_copy!(u16);
impl_copy!(u32);
impl_copy!(u64);
impl_copy!(u128);
impl_copy!(i8);
impl_copy!(i16);
impl_copy!(i32);
impl_copy!(i64);
impl_copy!(i128);
impl_copy!(f32);
impl_copy!(f64);

impl_copy!(::core::num::NonZeroUsize);
impl_copy!(::core::num::NonZeroIsize);
impl_copy!(::core::num::NonZeroU8);
impl_copy!(::core::num::NonZeroU16);
impl_copy!(::core::num::NonZeroU32);
impl_copy!(::core::num::NonZeroU64);
impl_copy!(::core::num::NonZeroU128);
impl_copy!(::core::num::NonZeroI8);
impl_copy!(::core::num::NonZeroI16);
impl_copy!(::core::num::NonZeroI32);
impl_copy!(::core::num::NonZeroI64);
impl_copy!(::core::num::NonZeroI128);

#[cfg(feature = "alloc")]
impl<T> TryClone for ::rust_alloc::boxed::Box<T>
where
    T: TryClone,
{
    fn try_clone(&self) -> Result<Self, Error> {
        Ok(::rust_alloc::boxed::Box::new(self.as_ref().try_clone()?))
    }
}

#[cfg(feature = "alloc")]
impl<T> TryClone for ::rust_alloc::boxed::Box<[T]>
where
    T: TryClone,
{
    fn try_clone(&self) -> Result<Self, Error> {
        // TODO: use a fallible box allocation.
        let mut out = ::rust_alloc::vec::Vec::with_capacity(self.len());

        for value in self.iter() {
            out.push(value.try_clone()?);
        }

        Ok(out.into())
    }
}

#[cfg(feature = "alloc")]
impl TryClone for ::rust_alloc::string::String {
    #[inline]
    fn try_clone(&self) -> Result<Self, Error> {
        // TODO: use fallible allocations for component.
        Ok(self.clone())
    }
}

#[cfg(all(test, feature = "alloc"))]
impl<T> TryClone for ::rust_alloc::vec::Vec<T>
where
    T: TryClone,
{
    #[inline]
    fn try_clone(&self) -> Result<Self, Error> {
        let mut out = ::rust_alloc::vec::Vec::with_capacity(self.len());

        for value in self {
            out.push(value.try_clone()?);
        }

        Ok(out)
    }
}
