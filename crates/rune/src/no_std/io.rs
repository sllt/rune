use core::fmt;

use crate::no_std::error::Error as StdError;
use crate::no_std::vec::Vec;

#[derive(Debug)]
#[non_exhaustive]
pub struct Error {}

impl Error {
    pub(crate) const fn new() -> Self {
        Self {}
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I/O operations are not supported in no_std environments")
    }
}

impl StdError for Error {}

pub(crate) type Result<T> = ::core::result::Result<T, Error>;

/// A trait for objects which are byte-oriented sinks.
pub trait Write {
    /// Attempts to write an entire buffer into this writer.
    fn write(&mut self, buf: &[u8]) -> Result<usize>;

    /// Flush data being written.
    fn flush(&mut self) -> Result<()>;

    /// Attempts to write an entire buffer into this writer.
    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => {
                    return Err(Error {});
                }
                Ok(n) => buf = &buf[n..],
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> Result<()> {
        // Create a shim which translates a Write to a fmt::Write and saves
        // off I/O errors. instead of discarding them
        struct Adapter<'a, T: ?Sized + 'a> {
            inner: &'a mut T,
            error: Result<()>,
        }

        impl<T: Write + ?Sized> fmt::Write for Adapter<'_, T> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                match self.inner.write_all(s.as_bytes()) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        self.error = Err(e);
                        Err(fmt::Error)
                    }
                }
            }
        }

        let mut output = Adapter {
            inner: self,
            error: Ok(()),
        };

        match fmt::write(&mut output, fmt) {
            Ok(()) => Ok(()),
            Err(..) => {
                // check if the error came from the underlying `Write` or not
                if output.error.is_err() {
                    output.error
                } else {
                    Err(Error::new())
                }
            }
        }
    }
}

impl Write for Vec<u8> {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.extend_from_slice(buf);
        Ok(buf.len())
    }

    #[inline]
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.extend_from_slice(buf);
        Ok(())
    }
}
