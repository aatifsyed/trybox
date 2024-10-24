use core::{
    alloc::{GlobalAlloc, Layout},
    ptr,
    sync::atomic::AtomicBool,
};
use std::{io, sync::atomic::Ordering};

use expect_test::{expect_file, ExpectFile};
use libtest_mimic::{Arguments, Trial};

#[global_allocator]
static ALLOC: FailOrFallback = FailOrFallback::system();

fn main() {
    let mut args = Arguments::from_args();
    args.test_threads = Some(1);
    libtest_mimic::run(
        &args,
        vec![
            error_message(
                "i32-error-message",
                expect_file!["i32-error-message.expected"],
                1i32,
            ),
            error_message(
                "2k-error-message",
                expect_file!["2k-error-message.expected"],
                [0u8; 2048],
            ),
            error_message(
                "2.5k-error-message",
                expect_file!["2.5k-error-message.expected"],
                [0u8; 2500],
            ),
            Trial::test("io-error-kind", || {
                let e: io::Error = fail_alloc(1).into();
                assert_eq!(e.kind(), io::ErrorKind::OutOfMemory);
                let e: io::ErrorKind = fail_alloc(1).into();
                assert_eq!(e, io::ErrorKind::OutOfMemory);
                Ok(())
            }),
        ],
    )
    .exit()
}

fn error_message<T: Send + 'static>(name: &str, file: ExpectFile, x: T) -> Trial {
    Trial::test(name, move || {
        let err = fail_alloc(x).to_string();
        file.assert_eq(&err);
        Ok(())
    })
}

fn fail_alloc<T>(x: T) -> try_box::Error<T> {
    ALLOC.fail();
    let Err(err) = try_box::new(x) else {
        unreachable!("we've made the allocator start failing")
    };
    ALLOC.fallback();
    err
}

#[derive(Debug, Default)]
pub struct FailOrFallback<T = std::alloc::System> {
    fail: AtomicBool,
    fallback: T,
}

impl FailOrFallback {
    /// See [new](FailOrFallback::new).
    pub const fn system() -> Self {
        Self::new(std::alloc::System)
    }
}

impl<T> FailOrFallback<T> {
    /// Create a new allocator, which uses the fallback by default.
    ///
    /// This behaviour allows e.g the rust runtime to initialize when using
    /// `#[global_allocator]`
    pub const fn new(fallback: T) -> Self {
        FailOrFallback {
            fail: AtomicBool::new(false),
            fallback,
        }
    }
    /// Allocations after this call will always fail.
    pub fn fail(&self) {
        self.fail.store(true, Ordering::Release);
    }
    /// Allocations after this call will use the fallback allocator.
    pub fn fallback(&self) {
        self.fail.store(false, Ordering::Release);
    }
}

unsafe impl GlobalAlloc for FailOrFallback {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match self.fail.load(Ordering::Acquire) {
            true => ptr::null_mut(),
            false => self.fallback.alloc(layout),
        }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.fallback.dealloc(ptr, layout);
    }
}
