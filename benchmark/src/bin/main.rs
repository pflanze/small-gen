use std::{ops::Range, time::SystemTime};

use small_gen::fast::generate;

fn fibs() -> impl Iterator<Item = u128> {
    generate(async move |co| {
        let mut n1 = 1;
        let mut n2 = 1;
        loop {
            let n = n1 + n2;
            n1 = n2;
            n2 = n;
            co.yield_(n).await;
        }
    })
}

fn series() -> impl Iterator<Item = u128> {
    generate(async move |co| {
        let mut fibs = fibs();
        loop {
            let fib = fibs.next().unwrap();
            for i in 0..1000000 {
                co.yield_(fib * i).await;
            }
        }
    })
}

struct NativeFibs {
    n1: u128,
    n2: u128,
}

impl NativeFibs {
    fn new() -> Self {
        Self { n1: 1, n2: 1 }
    }
}

impl Iterator for NativeFibs {
    type Item = u128;

    fn next(&mut self) -> Option<Self::Item> {
        let Self { n1, n2 } = self;
        let n = *n1 + *n2;
        *n1 = *n2;
        *n2 = n;
        Some(n)
    }
}

struct NativeSeries {
    fibs: NativeFibs,
    fib: u128,
    i_range: Range<u128>,
}

fn new_i_range() -> Range<u128> {
    0..1000000
}

impl NativeSeries {
    fn new() -> Self {
        let mut fibs = NativeFibs::new();
        let fib = fibs.next().unwrap();
        Self {
            fibs,
            fib,
            i_range: new_i_range(),
        }
    }
}

impl Iterator for NativeSeries {
    type Item = u128;

    fn next(&mut self) -> Option<Self::Item> {
        let Self { fibs, fib, i_range } = self;
        let i = if let Some(i) = i_range.next() {
            i
        } else {
            *fib = fibs.next().unwrap();
            *i_range = new_i_range();
            i_range.next().unwrap()
        };
        Some(*fib * i)
    }
}

fn bench(f: impl FnOnce() -> ()) -> f64 {
    let start = SystemTime::now();
    f();
    let end = SystemTime::now();
    let dur = end.duration_since(start).expect("in range");
    dur.as_secs_f64()
}

fn main() {
    dbg!(bench(|| {
        for n in series().skip(100000000).take(10) {
            dbg!(n);
        }
    }));

    dbg!(bench(|| {
        for n in NativeSeries::new().skip(100000000).take(10) {
            dbg!(n);
        }
    }));
}
