#![allow(clippy::missing_safety_doc)]

pub mod raw;
mod scopeguard;

pub use self::map::HashMap;
pub mod map;

pub use self::set::HashSet;
pub mod set;

use super::CustomError;
use core::marker::PhantomData;

/// Trait used to implement custom equality implementations which are not solely
/// based on traits.
pub trait EqFn<C: ?Sized, T: ?Sized, E> {
    fn eq(&self, cx: &mut C, key: &T) -> Result<bool, E>;

    #[doc(hidden)]
    fn into_tuple<V>(self) -> TupleFn<Self, V>
    where
        Self: Sized,
    {
        TupleFn {
            this: self,
            _marker: PhantomData,
        }
    }
}

impl<U, C: ?Sized, T: ?Sized, E> EqFn<C, T, E> for U
where
    U: Fn(&mut C, &T) -> Result<bool, E>,
{
    #[inline]
    fn eq(&self, cx: &mut C, key: &T) -> Result<bool, E> {
        self(cx, key)
    }
}

/// Trait used to implement custom hash implementations which are not solely
/// based on traits.
pub trait HasherFn<C: ?Sized, T: ?Sized, E> {
    fn hash(&self, cx: &mut C, key: &T) -> Result<u64, E>;

    #[doc(hidden)]
    fn into_tuple<V>(self) -> TupleFn<Self, V>
    where
        Self: Sized,
    {
        TupleFn {
            this: self,
            _marker: PhantomData,
        }
    }
}

impl<U, C: ?Sized, T: ?Sized, E> HasherFn<C, T, E> for U
where
    U: Fn(&mut C, &T) -> Result<u64, E>,
{
    #[inline]
    fn hash(&self, cx: &mut C, key: &T) -> Result<u64, E> {
        self(cx, key)
    }
}

/// Adapter for [`HasherFn`] for hashing tuples.
pub struct TupleFn<T, V> {
    this: T,
    _marker: PhantomData<V>,
}

impl<T, C: ?Sized, K, V, E> EqFn<C, (K, V), E> for TupleFn<T, V>
where
    T: EqFn<C, K, E>,
{
    #[inline]
    fn eq(&self, cx: &mut C, (key, _): &(K, V)) -> Result<bool, E> {
        self.this.eq(cx, key)
    }
}

impl<T, C: ?Sized, K, V, E> HasherFn<C, (K, V), E> for TupleFn<T, V>
where
    T: HasherFn<C, K, E>,
{
    #[inline]
    fn hash(&self, cx: &mut C, (key, _): &(K, V)) -> Result<u64, E> {
        self.this.hash(cx, key)
    }
}

/// Error raised by [`RawTable::find_or_find_insert_slot`].
///
/// [`RawTable::find_or_find_insert_slot`]:
///     crate::hashbrown::raw::RawTable::find_or_find_insert_slot
pub enum ErrorOrInsertSlot<E> {
    /// An error was returned.
    Error(CustomError<E>),
    /// A return slot was inserted.
    InsertSlot(raw::InsertSlot),
}

impl<E> From<CustomError<E>> for ErrorOrInsertSlot<E> {
    #[inline]
    fn from(error: CustomError<E>) -> Self {
        Self::Error(error)
    }
}

/// Key equivalence trait.
///
/// This trait defines the function used to compare the input value with the map
/// keys (or set values) during a lookup operation such as [`HashMap::get`] or
/// [`HashSet::contains`]. It is provided with a blanket implementation based on
/// the [`Borrow`](core::borrow::Borrow) trait.
///
/// # Correctness
///
/// Equivalent values must hash to the same value.
pub trait Equivalent<K: ?Sized> {
    /// Checks if this value is equivalent to the given key.
    ///
    /// Returns `true` if both values are equivalent, and `false` otherwise.
    ///
    /// # Correctness
    ///
    /// When this function returns `true`, both `self` and `key` must hash to
    /// the same value.
    fn equivalent(&self, key: &K) -> bool;
}

impl<Q: ?Sized, K: ?Sized> Equivalent<K> for Q
where
    Q: Eq,
    K: core::borrow::Borrow<Q>,
{
    fn equivalent(&self, key: &K) -> bool {
        self == key.borrow()
    }
}
