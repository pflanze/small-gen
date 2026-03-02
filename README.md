# Generators for Rust

This is a fork of
[rust_async_generators](https://github.com/tbfleming/rust_async_generators)
with changes to make it production ready.

It has the following goals:

* Base its API around `async/await`.
* No macros in its API.
* Not use threads in its implementation or require them in its API.
* No `unsafe` code, for now (might add some for further performance improvements).
* In versions that allow the generator to move between threads, and that doesn't but it faster instead.

## Example: generate an infinite sequence

```rust
// use `small_gen::sync::generate` instead if you need to work across threads
use small_gen::fast::generate;

// Generate [0, 1, 2, 3, ...]
let iter = generate(|co| async move {
    for i in 0.. {
        co.yield_(i * 5).await;
    }
});

for j in iter.take(10) {
    // 0, 5, 10, 15, 20, 25, 30, 35, 40, 45
    println!("j = {j}");
}

// The for loop consumed the iterator. This canceled the
// infinite loop in the async block after it yielded `45`
// and dropped (cleaned up) its state.
```

## Example: return an error from the generator

```
// use `small_gen::sync::try_generate` instead if you need to work across threads
use small_gen::fast::try_generate;

let mut iter = try_generate(async move |co| -> Result<(), &'static str> {
    co.yield_(0).await;
    co.yield_(1).await;
    Err("fail")?;
    co.yield_(2).await;
    Ok(())
});

assert_eq!(iter.next(), Some(Ok(0)));
assert_eq!(iter.next(), Some(Ok(1)));
assert_eq!(iter.next(), Some(Err("fail")));
assert_eq!(iter.next(), None);
assert_eq!(iter.next(), None);
```
