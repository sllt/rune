use crate::alloc::{Allocator, Box, Error, Global, String, TryToOwned};

/// Converts a boxed slice of bytes to a boxed string slice without checking
/// that the string contains valid UTF-8.
///
/// # Examples
///
/// ```
/// use rune_alloc::Box;
/// use rune_alloc::str;
///
/// let smile_utf8 = Box::try_from([226, 152, 186])?;
/// let smile = unsafe { str::from_boxed_utf8_unchecked(smile_utf8) };
///
/// assert_eq!("☺", &*smile);
/// # Ok::<_, rune_alloc::Error>(())
/// ```
///
/// # Safety
///
/// The provided buffer must be valid UTF-8.
#[must_use]
#[inline]
pub unsafe fn from_boxed_utf8_unchecked<A: Allocator>(v: Box<[u8], A>) -> Box<str, A> {
    let (ptr, alloc) = Box::into_raw_with_allocator(v);
    unsafe { Box::from_raw_in(ptr as *mut str, alloc) }
}

/// Converts a [`Box<str>`] into a [`String`] without copying or allocating.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// use rune_alloc::{String, TryClone};
/// use rune_alloc::str;
///
/// let string = String::try_from("birthday gift")?;
/// let boxed_str = string.try_clone()?.try_into_boxed_str()?;
///
/// assert_eq!(str::into_string(boxed_str), string);
/// # Ok::<_, rune_alloc::Error>(())
/// ```
#[must_use = "`self` will be dropped if the result is not used"]
#[inline]
pub fn into_string<A: Allocator>(this: Box<str, A>) -> String<A> {
    let slice = Box::<[u8], A>::from(this);
    let vec = crate::alloc::slice::into_vec(slice);
    unsafe { String::<A>::from_utf8_unchecked(vec) }
}

impl TryToOwned for str {
    type Owned = String<Global>;

    #[inline]
    fn try_to_owned(&self) -> Result<String<Global>, Error> {
        Ok(unsafe { String::from_utf8_unchecked(self.as_bytes().try_to_owned()?) })
    }
}
