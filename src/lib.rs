//! Stable, `no_std`-compatible, fallible heap allocation for [`Box`].
//!
//! Basic usage is as follows:
//! ```
//! match try_box::new(1) {
//!     Ok(heaped) => {
//!         let _: Box<i32> = heaped;
//!     }
//!     Err(e) => panic!("couldn't allocate {}", e.payload),
//!                  // access the underlying item ^
//! }
//! ```
//!
//! You may drop the object after allocation failure instead,
//! which is useful for wrapping in custom error types or casting as trait objects:
//!
//! ```
//! fn fallible<T>(x: T) -> Result<Box<T>, Box<dyn std::error::Error + Send + Sync>> {
//!         // doesn't contain `payload`, so is always thread-safe etc ^^^^^^^^^^^
//!     Ok(try_box::or_drop(x)?)
//! }
//! ```
//!
//! Care has been taken to optimize the size of [`Error`] down to a single usize:
//! ```
//! # use std::mem::size_of;
//! assert_eq!(size_of::<try_box::Error>(), size_of::<usize>());
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
//!     Ok(try_box::new(x)?)
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
/// returning `x` wrapped in an [`Error`] on failure.
///
/// See [crate documentation](mod@self) for more.
#[inline(always)]
pub fn new<T>(x: T) -> Result<Box<T>, Error<T>> {
    match imp(x) {
        Ok(it) => Ok(it),
        Err(payload) => Err(Error {
            payload,
            info: T::info,
        }),
    }
}

/// Attempt to move `x` to a heap allocation,
/// immediately dropping `x` on failure.
///
/// The returned [`Error`] is always suitable for propogation.
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
                    Ok(unsafe { Box::<MaybeUninit<T>>::assume_init(heap) })
                }
            }
        }
    }
}

/// Represents an allocation failure,
/// possibly containing the [payload](Self::payload) that the allocation failed for.
///
/// If [`Self::without_payload`] is called,
/// this is guaranteed to take up a single machine word.
pub struct Error<T = ()> {
    pub payload: T,
    // This could be replaced by `&'static Info` once type_name is a const fn
    info: fn() -> Info,
}

impl<T: fmt::Debug> fmt::Debug for Error<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Info { layout, name } = self.info();
        let mut d = f.debug_struct("Error");
        d.field("layout", &layout).field("name", &name);
        if any::type_name::<()>() != any::type_name::<T>() {
            d.field("payload", &self.payload);
        }
        d.finish()
    }
}

impl<T> fmt::Display for Error<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (prefix, precision, num) = match NumberPrefix::binary(self.layout().size() as f64) {
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
            "memory allocation of {num:.precision$} {prefix}bytes (for type {}) failed",
            self.info().name
        ))
    }
}

impl core::error::Error for Error {}

impl<T> Error<T> {
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
    ///
    /// This is not the same as calling [`Layout::for_value`] on [`Self::payload`]
    /// because the payload may have been replaced e.g using [`Self::without_payload`].
    #[inline(always)]
    pub fn layout(&self) -> Layout {
        self.info().layout
    }
    /// Immediately drops the item that failed to allocate,
    /// but retain the actual context to display in the error.
    #[inline(always)]
    pub fn without_payload(self) -> Error {
        let Self { payload: _, info } = self;
        Error { payload: (), info }
    }
}

#[cfg(feature = "std")]
impl<T> From<Error<T>> for std::io::Error {
    /// Create an [`OutOfMemory`](std::io::ErrorKind::OutOfMemory) error,
    /// possibly with an [`Error`] as the [source](std::error::Error::source),
    /// discarding the [payload](Error::payload).
    fn from(value: Error<T>) -> Self {
        let kind = std::io::ErrorKind::OutOfMemory;

        // Creating a new io::Error with a source involves a heap allocation,
        // but we're probably in a memory-constrained scenario,
        // so _try_ and preserve the source,
        // or just use an io::ErrorKind if we can't.
        match or_drop(value.without_payload()) {
            Ok(source) => {
                std::io::Error::new(kind, source as Box<dyn std::error::Error + Send + Sync>)
            }
            Err(_cannot_preserve) => std::io::Error::from(kind),
        }
    }
}

#[cfg(feature = "std")]
impl<T> From<Error<T>> for std::io::ErrorKind {
    fn from(_: Error<T>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_eq_size!(Error, *const u8);
    static_assertions::assert_impl_all!(Error: Send, Sync);
}
