// Todd Fleming 2023

//! Basic implementation of async generators.
//!
//! [generate] turns an async function into a fully-synchronous [Iterator].
//!
//! ## Example
//!
//! ```
//! use gen::generate;
//!
//! // Create an iterator. The argument `co` allows the async block to
//! // send items to the iterator. Everything runs on a single thread.
//! let iter = generate(|co| async move {
//!     // This code is in an async block that's within a closure.
//!     // `generate` executes the closure immediately. The closure
//!     // returns a `Future` for the async block. The first call to
//!     // `Iterator::next` starts executing the async block.
//!     for i in 0..4 {
//!         // The code which uses the iterator is currently blocked in
//!         // `Iterator::next`. Send a single value to the iterator.
//!         // `await` blocks this code and unblocks `Iterator::next`.
//!         // This code resumes executing the next time `Iterator::next`
//!         // is called, if ever.
//!         co.yield_(i).await;
//!     }
//!
//!     // If the async code ever returns like it does here, then
//!     // `Iterator::next` returns `None`. This ends the loop below.
//! });
//!
//! for j in iter {
//!     println!("j = {j}");
//! }
//!
//! println!("done");
//! ```

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake},
};

use lazy_static::lazy_static;

// Shared state between Communication and Generator.
//
// Rc<RefCell<Option<Item>>> would work, but would prevent
// Generator from being able to move between threads.
type SharedState<Item> = Arc<Mutex<Option<Item>>>;

/// An iterator which synchronously produces items yielded by an async function.
///
/// [generate] returns this. See [crate documentation](crate) for usage.
pub struct Generator<Item, Fut: Future<Output = ()>> {
    shared: SharedState<Item>,
    future: Pin<Box<Fut>>,
    done: bool,
}

/// An iterator which synchronously produces Result items yielded by
/// an async function that also returns a Result (hence can use `?`).
///
/// [generate] returns this. See [crate documentation](crate) for usage.
pub struct TryGenerator<Item, E, Fut: Future<Output = Result<(), E>>> {
    shared: SharedState<Item>,
    future: Pin<Box<Fut>>,
    done: bool,
}

/// Turn an async function into a fully-synchronous [Iterator].
///
/// See [crate documentation](crate) for usage.
pub fn generate<Item, F, Fut>(f: F) -> Generator<Item, Fut>
where
    F: FnOnce(Communication<Item>) -> Fut,
    Fut: Future<Output = ()>,
{
    let shared: SharedState<Item> = Default::default();
    let future = Box::pin(f(Communication(shared.clone())));
    Generator {
        shared,
        future,
        done: false,
    }
}

pub fn try_generate<Item, E, F, Fut>(f: F) -> TryGenerator<Item, E, Fut>
where
    F: FnOnce(Communication<Item>) -> Fut,
    Fut: Future<Output = Result<(), E>>,
{
    let shared: SharedState<Item> = Default::default();
    let future = Box::pin(f(Communication(shared.clone())));
    TryGenerator {
        shared,
        future,
        done: false,
    }
}

struct Waker;

impl Wake for Waker {
    fn wake(self: Arc<Self>) {}
}

lazy_static! {
    static ref WAKER: std::task::Waker = Arc::new(Waker).into();
}

impl<Item, Fut: Future<Output = ()>> Iterator for Generator<Item, Fut> {
    type Item = Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        // Execute future until it yields a new value or finishes.
        loop {
            let poll = self.future.as_mut().poll(&mut Context::from_waker(&WAKER));

            match poll {
                Poll::Ready(()) => {
                    self.done = true;
                    return None;
                }
                Poll::Pending => {
                    let out = self.shared.lock().unwrap().take();
                    if out.is_some() {
                        return out;
                    }
                }
            }
        }
    }
}

impl<Item, E, Fut: Future<Output = Result<(), E>>> Iterator for TryGenerator<Item, E, Fut> {
    type Item = Result<Item, E>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        // Execute future until it yields a new value or finishes.
        loop {
            let poll = self.future.as_mut().poll(&mut Context::from_waker(&WAKER));

            match poll {
                Poll::Ready(val) => {
                    self.done = true;
                    return match val {
                        Ok(()) => None,
                        Err(e) => Some(Err(e)),
                    };
                }
                Poll::Pending => {
                    let out = self.shared.lock().unwrap().take();
                    if out.is_some() {
                        return Ok(out).transpose();
                    }
                }
            }
        }
    }
}

/// Communicate with [Generator]
///
/// The function passed to `generate` receives this as an
/// argument. It uses this to pass items to [Generator].
///
/// This type could have also been named Coroutine, but
/// I thought it better to reserve that name for the async
/// function.
pub struct Communication<Item>(SharedState<Item>);

impl<Item> Communication<Item> {
    /// Pass a single value to [Generator]. `yield_` acts as
    /// an async function.
    pub fn yield_(&self, item: Item) -> YieldFuture {
        let mut lock = self.0.lock().unwrap();
        lock.replace(item);
        YieldFuture { done: false }
    }
}

/// Future returned by [Communication::yield_]
pub struct YieldFuture {
    done: bool,
}

// YieldFuture doesn't point to itself
impl Unpin for YieldFuture {}

impl Future for YieldFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Why does it need this two-call hack?
        let this = self.get_mut();
        if this.done {
            Poll::Ready(())
        } else {
            this.done = true;
            Poll::Pending
        }
    }
}
