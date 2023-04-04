use crate::runtime::{Mut, Ref, TypeInfo};
use crate::Hash;

/// Full type information.
#[derive(Clone)]
pub struct FullTypeOf {
    pub(crate) hash: Hash,
    #[cfg(feature = "doc")]
    pub(crate) type_info: TypeInfo,
}

/// Trait used for Rust types for which we can determine the runtime type of.
pub trait TypeOf {
    /// Type information for the given type.
    #[inline]
    fn type_of() -> FullTypeOf {
        FullTypeOf {
            hash: Self::type_hash(),
            #[cfg(feature = "doc")]
            type_info: Self::type_info(),
        }
    }

    /// Convert into a type hash.
    fn type_hash() -> Hash;

    /// Access diagnostical information on the value type.
    fn type_info() -> TypeInfo;
}

/// Blanket implementation for references.
impl<T> TypeOf for &T
where
    T: ?Sized + TypeOf,
{
    #[inline]
    fn type_hash() -> Hash {
        T::type_hash()
    }

    #[inline]
    fn type_info() -> TypeInfo {
        T::type_info()
    }
}

/// Blanket implementation for mutable references.
impl<T> TypeOf for &mut T
where
    T: ?Sized + TypeOf,
{
    #[inline]
    fn type_hash() -> Hash {
        T::type_hash()
    }

    #[inline]
    fn type_info() -> TypeInfo {
        T::type_info()
    }
}

/// Blanket implementation for owned references.
impl<T> TypeOf for Ref<T>
where
    T: ?Sized + TypeOf,
{
    #[inline]
    fn type_hash() -> Hash {
        T::type_hash()
    }

    #[inline]
    fn type_info() -> TypeInfo {
        T::type_info()
    }
}

/// Blanket implementation for owned mutable references.
impl<T> TypeOf for Mut<T>
where
    T: ?Sized + TypeOf,
{
    #[inline]
    fn type_hash() -> Hash {
        T::type_hash()
    }

    #[inline]
    fn type_info() -> TypeInfo {
        T::type_info()
    }
}
