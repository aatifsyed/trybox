Stable, `no_std`-compatible, fallible heap allocation for [`Box`].

Basic usage is as follows:
```rust
match trybox::new(1) {
    Ok(heaped) => {
        let _: Box<i32> = heaped;
    }
    Err(ErrorWith(stacked)) => {
        let _: i32 = stacked; // failed object is returned on the stack
    },
}
```

You may drop the object after allocation failure instead,
choosing to e.g propogate or wrap the [`Error`].

```rust
fn fallible<T>(x: T) -> Result<Box<T>, Box<dyn std::error::Error + Send + Sync>> {
    Ok(trybox::or_drop(x)?)
}
```

Care has been taken to optimize the size of [`Error`] down to a single usize:
```rust
assert_eq!(size_of::<trybox::Error>(), size_of::<usize>());
```

And to provide ergonomic error messages:
```text
memory allocation of 4 bytes (for type i32) failed
```
```text
memory allocation of 2.44 kibibytes (for type [u8; 2500]) failed
```

Conversions to [`std::io::Error`] and [`std::io::ErrorKind::OutOfMemory`]
are provided when the `"std"` feature is enabled:

```rust
fn fallible<T>(x: T) -> std::io::Result<Box<T>> {
    Ok(trybox::or_drop(x)?)
}
```

# Comparison with other crates
- [`fallacy-box`](https://docs.rs/fallacy-box/0.1.1/fallacy_box/)
  - [requires a nightly compiler](https://docs.rs/fallacy-box/0.1.1/src/fallacy_box/lib.rs.html#3).
- [`fallible_collections`](https://docs.rs/fallible_collections/0.4.9/fallible_collections/)
  - You must use either the [`TryBox`](https://docs.rs/fallible_collections/0.4.9/fallible_collections/enum.TryReserveError.html)
    wrapper struct, or the [`FallibleBox`](https://docs.rs/fallible_collections/0.4.9/fallible_collections/boxed/trait.FallibleBox.html)
    extension trait.
  - The [returned error type](https://docs.rs/fallible_collections/0.4.9/fallible_collections/enum.TryReserveError.html)
    doesn't implement common error traits, and isn't strictly minimal.
