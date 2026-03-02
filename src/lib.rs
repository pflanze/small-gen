// Todd Fleming 2023 & Christian Jaeger 2026

//! Basic implementation of async generators.
//!
//! ## Example
//!
//! ```
//! // use `small_gen::sync::generate` instead if you need to work across threads
//! use small_gen::fast::generate;
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
//! // To create generators that can fail, use `try_generate`
//! use small_gen::fast::try_generate;
//!
//! let mut iter = try_generate(async move |co| -> Result<(), &'static str> {
//!     co.yield_(0).await;
//!     co.yield_(1).await;
//!     Err("fail")?;
//!     co.yield_(2).await;
//!     Ok(())
//! });
//!
//! assert_eq!(iter.next(), Some(Ok(0)));
//! assert_eq!(iter.next(), Some(Ok(1)));
//! assert_eq!(iter.next(), Some(Err("fail")));
//! assert_eq!(iter.next(), None);
//! assert_eq!(iter.next(), None);
//! ```

use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake},
};

use lazy_static::lazy_static;

// Shared state between Communication and Generator.

pub trait SharedState: Default + Clone {
    type Item;
    fn new() -> Self;
    fn take(&self) -> Option<Self::Item>;
    fn set(&self, val: Self::Item);
}

pub struct FastSharedState<Item>(Rc<Cell<Option<Item>>>);

impl<Item> Default for FastSharedState<Item> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Item> Clone for FastSharedState<Item> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<Item> SharedState for FastSharedState<Item> {
    type Item = Item;

    fn new() -> Self {
        Self(Rc::new(Cell::new(None)))
    }

    fn take(&self) -> Option<Self::Item> {
        self.0.take()
    }

    fn set(&self, val: Self::Item) {
        self.0.set(Some(val));
    }
}

pub struct SyncSharedState<Item>(Arc<Mutex<Option<Item>>>);

impl<Item> Default for SyncSharedState<Item> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<Item> Clone for SyncSharedState<Item> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<Item> SharedState for SyncSharedState<Item> {
    type Item = Item;

    fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    fn take(&self) -> Option<Self::Item> {
        self.0.lock().expect("no panics").take()
    }

    fn set(&self, val: Self::Item) {
        _ = self.0.lock().expect("no panics").insert(val);
    }
}

/// An iterator which synchronously produces items yielded by an async function.
///
/// [fast::generate] and [sync::generate] return this. See [crate
/// documentation](crate) for usage.
pub struct Generator<Item, St: SharedState<Item = Item>, Fut: Future<Output = ()>> {
    shared: St,
    future: Pin<Box<Fut>>,
    done: bool,
}

/// An iterator which synchronously produces Result items yielded by
/// an async function that also returns a Result (hence can use `?` in
/// its body).
///
/// [fast::try_generate] and [sync::try_generate] return this. See
/// [crate documentation](crate) for usage.
pub struct TryGenerator<Item, E, St: SharedState<Item = Item>, Fut: Future<Output = Result<(), E>>>
{
    shared: St,
    future: Pin<Box<Fut>>,
    done: bool,
}

pub mod fast {
    use std::future::Future;

    use crate::{Communication, FastSharedState, Generator, TryGenerator};

    /// Turn an async function receiving a value to `yield` on into an
    /// [Iterator]. The returned iterator is not `Sync`. For a version
    /// that returns an iterator that is `Sync` but slower, see
    /// [crate::sync::generate].
    ///
    /// See [crate documentation](crate) for usage.
    pub fn generate<Item, F, Fut>(f: F) -> Generator<Item, FastSharedState<Item>, Fut>
    where
        F: FnOnce(Communication<Item, FastSharedState<Item>>) -> Fut,
        Fut: Future<Output = ()>,
    {
        let shared: FastSharedState<Item> = Default::default();
        let future = Box::pin(f(Communication(shared.clone())));
        Generator {
            shared,
            future,
            done: false,
        }
    }

    /// Like `generate` but creating an iterator that returns `Result`
    /// values. An `Err` returned from the function is returned from
    /// the iterator, while values passed to the `yield` method are
    /// returned from the iterator as `Ok`. This means the function
    /// can use `?` in its body.
    pub fn try_generate<Item, E, F, Fut>(f: F) -> TryGenerator<Item, E, FastSharedState<Item>, Fut>
    where
        F: FnOnce(Communication<Item, FastSharedState<Item>>) -> Fut,
        Fut: Future<Output = Result<(), E>>,
    {
        let shared: FastSharedState<Item> = Default::default();
        let future = Box::pin(f(Communication(shared.clone())));
        TryGenerator {
            shared,
            future,
            done: false,
        }
    }
}

pub mod sync {
    use std::future::Future;

    use crate::{Communication, Generator, SyncSharedState, TryGenerator};

    /// Turn an async function receiving a value to `yield` on into an
    /// [Iterator]. The returned iterator is `Sync`. For a version
    /// that returns an iterator that is not, but faster, see
    /// [crate::fast::generate].
    ///
    /// See [crate documentation](crate) for usage.
    pub fn generate<Item, F, Fut>(f: F) -> Generator<Item, SyncSharedState<Item>, Fut>
    where
        F: FnOnce(Communication<Item, SyncSharedState<Item>>) -> Fut,
        Fut: Future<Output = ()>,
    {
        let shared: SyncSharedState<Item> = Default::default();
        let future = Box::pin(f(Communication(shared.clone())));
        Generator {
            shared,
            future,
            done: false,
        }
    }

    /// Like `generate` but creating an iterator that returns `Result`
    /// values. An `Err` returned from the function is returned from
    /// the iterator, while values passed to the `yield` method are
    /// returned from the iterator as `Ok`. This means the function
    /// can use `?` in its body.
    pub fn try_generate<Item, E, F, Fut>(f: F) -> TryGenerator<Item, E, SyncSharedState<Item>, Fut>
    where
        F: FnOnce(Communication<Item, SyncSharedState<Item>>) -> Fut,
        Fut: Future<Output = Result<(), E>>,
    {
        let shared: SyncSharedState<Item> = Default::default();
        let future = Box::pin(f(Communication(shared.clone())));
        TryGenerator {
            shared,
            future,
            done: false,
        }
    }
}

struct Waker;

impl Wake for Waker {
    fn wake(self: Arc<Self>) {}
    fn wake_by_ref(self: &Arc<Self>) {}
}

lazy_static! {
    static ref WAKER: std::task::Waker = Arc::new(Waker).into();
}

impl<Item, St: SharedState<Item = Item>, Fut: Future<Output = ()>> Iterator
    for Generator<Item, St, Fut>
{
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
                    let out = self.shared.take();
                    if out.is_some() {
                        return out;
                    }
                }
            }
        }
    }
}

impl<Item, E, St: SharedState<Item = Item>, Fut: Future<Output = Result<(), E>>> Iterator
    for TryGenerator<Item, E, St, Fut>
{
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
                    let out = self.shared.take();
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
pub struct Communication<Item, St: SharedState<Item = Item>>(St);

impl<Item, St: SharedState<Item = Item>> Communication<Item, St> {
    /// Pass a single value to [Generator]. `yield_` acts as
    /// an async function.
    pub fn yield_(&self, item: Item) -> YieldFuture {
        self.0.set(item);
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
