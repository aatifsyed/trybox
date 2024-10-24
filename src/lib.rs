//! Stable, `no_std`-compatible, fallible heap allocation for [`Box`].
//!
//! Basic usage is as follows:
//! ```
//! # use trybox::ErrorWith;
//! match trybox::new(1) {
//!     Ok(heaped) => {
//!         let _: Box<i32> = heaped;
//!     }
//!     Err(ErrorWith(stacked)) => {
//!         let _: i32 = stacked; // failed object is returned on the stack
//!     },
//! }
//! ```
//!
//! You may drop the object after allocation failure instead,
//! choosing to e.g propogate or wrap the [`Error`].
//!
//! ```
//! fn fallible<T>(x: T) -> Result<Box<T>, Box<dyn std::error::Error + Send + Sync>> {
//!     Ok(trybox::or_drop(x)?)
//! }
//! ```
//!
//! Care has been taken to optimize the size of [`Error`] down to a single usize:
//! ```
//! # use std::mem::size_of;
//! assert_eq!(size_of::<trybox::Error>(), size_of::<usize>());
//! ```
//!
//! And to provide ergonomic error messages:
//! ```text
#![doc = include_str!("../tests/i32-error-message.expected")]
//! ```
//! ```text
#![doc = include_str!("../tests/2.5k-error-message.expected")]
//! ```
//!
//! Conversions to [`std::io::Error`] and [`std::io::ErrorKind::OutOfMemory`]
//! are provided when the `"std"` feature is enabled:
//!
//! ```
//! fn fallible<T>(x: T) -> std::io::Result<Box<T>> {
//!     Ok(trybox::or_drop(x)?)
//! }
//! ```
//!
//! # Comparison with other crates
//! - [`fallacy-box`](https://docs.rs/fallacy-box/0.1.1/fallacy_box/)
//!   - [requires a nightly compiler](https://docs.rs/fallacy-box/0.1.1/src/fallacy_box/lib.rs.html#3).
//! - [`fallible_collections`](https://docs.rs/fallible_collections/0.4.9/fallible_collections/)
//!   - You must use either the [`TryBox`](https://docs.rs/fallible_collections/0.4.9/fallible_collections/enum.TryReserveError.html)
//!     wrapper struct, or the [`FallibleBox`](https://docs.rs/fallible_collections/0.4.9/fallible_collections/boxed/trait.FallibleBox.html)
//!     extension trait.
//!   - The [returned error type](https://docs.rs/fallible_collections/0.4.9/fallible_collections/enum.TryReserveError.html)
//!     doesn't implement common error traits, and isn't strictly minimal.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{
    alloc::{alloc, handle_alloc_error, Layout},
    boxed::Box,
};
use core::{any, fmt, mem::MaybeUninit, ptr::NonNull};

use number_prefix::NumberPrefix;

/// Attempt to move `x` to a heap allocation,
/// returning a wrapped `x` on failure.
///
/// See [crate documentation](mod@self) for more.
#[inline(always)]
pub fn new<T>(x: T) -> Result<Box<T>, ErrorWith<T>> {
    match imp(x) {
        Ok(it) => Ok(it),
        Err(e) => Err(ErrorWith(e)),
    }
}

/// Attempt to move `x` to a heap allocation,
/// immediately dropping `x` on failure,
/// and returning a useful [`Error`].
///
/// See [crate documentation](mod@self) for more.
#[inline(always)]
pub fn or_drop<T>(x: T) -> Result<Box<T>, Error> {
    match new(x) {
        Ok(it) => Ok(it),
        Err(e) => Err(e.without_payload()),
    }
}

#[inline(always)]
fn imp<T>(x: T) -> Result<Box<T>, T> {
    let layout = Layout::for_value(&x);
    match layout.size() == 0 {
        true => {
            let ptr = NonNull::<T>::dangling().as_ptr();
            // SAFETY: This is recommended by the Box documentation
            Ok(unsafe { Box::from_raw(ptr) })
        }
        false => {
            // SAFETY: We've checked layout to be non-empty, above.
            let ptr = unsafe { alloc(layout) }.cast::<T>();
            match ptr.is_null() {
                true => Err(x),
                false => {
                    // SAFETY:
                    // - we've called GlobalAlloc::alloc above.
                    // - Box::from_raw with such a pointer is explicitly called
                    //   out as safe in the Box docs.
                    let mut heap = unsafe { Box::<MaybeUninit<T>>::from_raw(ptr.cast()) };
                    heap.write(x);
                    // SAFETY: we've written an initialized T to the memory.
                    Ok(unsafe { Box::from_raw(Box::into_raw(heap).cast()) })
                }
            }
        }
    }
}

/// Represents an allocation failure from [`or_drop`].
///
/// Designed to be small and propogatable.
pub struct Error {
    // This could be replaced by `&'static Info` once type_name is a const fn
    info: fn() -> Info,
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Info { layout, name } = self.info();
        let mut d = f.debug_struct("Error");
        d.field("layout", &layout).field("name", &name);
        d.finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_info(self.info(), f)
    }
}

fn write_info(info: Info, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let Info { layout, name } = info;
    let (prefix, precision, num) = match NumberPrefix::binary(layout.size() as f64) {
        NumberPrefix::Standalone(num) => ("", 0, num),
        NumberPrefix::Prefixed(pre, num) => {
            #[cfg(feature = "std")]
            let precision = match num.fract() == 0.0 {
                true => 0,
                false => 2,
            };
            #[cfg(not(feature = "std"))]
            let precision = 2;
            (pre.lower(), precision, num)
        }
    };
    f.write_fmt(format_args!(
        "memory allocation of {num:.precision$} {prefix}bytes (for type {name}) failed",
    ))
}

#[cfg(not(feature = "std"))]
impl core::error::Error for Error {}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl Error {
    #[inline(always)]
    fn info(&self) -> Info {
        (self.info)()
    }
    /// Call [`handle_alloc_error`], typically aborting the process.
    ///
    /// See that function for more.
    #[inline(always)]
    pub fn handle(self) -> ! {
        handle_alloc_error(self.layout())
    }
    /// Get the [`Layout`] that corresponds to the failed allocation.
    #[inline(always)]
    pub fn layout(&self) -> Layout {
        self.info().layout
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::Error {
    /// Create an [`OutOfMemory`](std::io::ErrorKind::OutOfMemory) error,
    /// possibly with an [`Error`] as the [source](std::error::Error::source).
    fn from(value: Error) -> Self {
        let kind = std::io::ErrorKind::OutOfMemory;

        // Creating a new io::Error with a source involves a heap allocation,
        // but we're probably in a memory-constrained scenario,
        // so _try_ and preserve the source,
        // or just use an io::ErrorKind if we can't.
        match or_drop(value) {
            Ok(source) => {
                std::io::Error::new(kind, source as Box<dyn std::error::Error + Send + Sync>)
            }
            Err(_cannot_preserve) => std::io::Error::from(kind),
        }
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::ErrorKind {
    fn from(_: Error) -> Self {
        std::io::ErrorKind::OutOfMemory
    }
}

/// [`Layout`] is two words, but this function pointer is just one.
trait Indirect: Sized {
    fn info() -> Info {
        Info {
            layout: Layout::new::<Self>(),
            name: any::type_name::<Self>(),
        }
    }
}
impl<T: Sized> Indirect for T {}

#[derive(Debug, Clone, Copy)]
struct Info {
    layout: Layout,
    name: &'static str,
}

/// Represents the failure to allocate a particular object on the heap,
/// returned from [`new`].
#[derive(Debug)]
pub struct ErrorWith<T>(pub T);

impl<T> ErrorWith<T> {
    fn info(&self) -> Info {
        Info {
            layout: Layout::for_value(&self.0),
            name: any::type_name::<T>(),
        }
    }
    pub fn without_payload(self) -> Error {
        Error { info: T::info }
    }
}

impl<T> fmt::Display for ErrorWith<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_info(self.info(), f)
    }
}

#[cfg(not(feature = "std"))]
impl<T: fmt::Debug> core::error::Error for ErrorWith<T> {}

#[cfg(feature = "std")]
impl<T: fmt::Debug> std::error::Error for ErrorWith<T> {}

impl<T> From<ErrorWith<T>> for Error {
    fn from(value: ErrorWith<T>) -> Self {
        value.without_payload()
    }
}

#[cfg(feature = "std")]
impl<T> From<ErrorWith<T>> for std::io::Error {
    /// Create an [`OutOfMemory`](std::io::ErrorKind::OutOfMemory) error,
    /// possibly with an [`Error`] as the [source](std::error::Error::source).
    fn from(value: ErrorWith<T>) -> Self {
        Error::from(value).into()
    }
}

#[cfg(feature = "std")]
impl<T> From<ErrorWith<T>> for std::io::ErrorKind {
    fn from(_: ErrorWith<T>) -> Self {
        std::io::ErrorKind::OutOfMemory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_eq_size!(Error, *const u8);
    static_assertions::assert_impl_all!(Error: Send, Sync);
}
