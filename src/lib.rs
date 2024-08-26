#![cfg_attr(test, feature(test))]

use std::{
    cell::UnsafeCell,
    collections::BinaryHeap,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{RawWaker, RawWakerVTable, Waker},
};

pub mod wait;
#[cfg(feature = "sync")]
pub mod sync {
    pub use tokio::sync::*;
}

pub use tokio::{join, pin, try_join};

#[derive(Default)]
struct Runtime {
    current_tick: u64,
    futures: Vec<MyFuture>,
    on_tick: BinaryHeap<TickTimer>,
}

impl Runtime {
    #[cfg(test)]
    fn clear(&mut self) {
        self.current_tick = 0;
        self.futures.clear();
        self.on_tick.clear();
    }
}

thread_local! {
    static RT: std::cell::UnsafeCell<Runtime> = std::cell::UnsafeCell::new(Runtime::default());
}

#[inline(always)]
fn get_rt() -> &'static mut Runtime {
    RT.with(|rt| unsafe { &mut *rt.get() })
}

pub fn spawn<F>(f: F)
where
    F: Future<Output = ()> + 'static,
{
    get_rt().spawn(f);
}

pub fn tick() {
    get_rt().tick();
}

impl Runtime {
    pub fn spawn<F>(&mut self, f: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.futures
            .push(MyFuture(Rc::new(UnsafeCell::new(Box::pin(f)))));
    }

    pub fn tick(&mut self) {
        #[cfg(feature = "std")]
        {
            self.current_tick += 1;
        }
        #[cfg(feature = "lotus")]
        {
            self.current_tick = lotus_script::time::ticks_alive();
        }

        while let Some(timer) = self.on_tick.pop() {
            if timer.expires <= self.current_tick {
                (timer.callback)();
            } else {
                self.on_tick.push(timer);
                break;
            }
        }

        self.execute();
    }

    pub fn execute(&mut self) {
        while let Some(future) = self.futures.pop() {
            let waker = future.create_waker();

            let mut cx = std::task::Context::from_waker(&waker);
            let future = unsafe { &mut *future.0.get() };

            match future.as_mut().poll(&mut cx) {
                std::task::Poll::Ready(_) => {}
                std::task::Poll::Pending => {}
            }
        }
    }
}

struct TickTimer {
    expires: u64,
    callback: Box<dyn FnOnce()>,
}

impl PartialEq for TickTimer {
    fn eq(&self, other: &Self) -> bool {
        self.expires == other.expires
    }
}

impl Eq for TickTimer {}

impl PartialOrd for TickTimer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.expires.cmp(&other.expires))
    }
}

impl Ord for TickTimer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.expires.cmp(&other.expires)
    }
}

const RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |p| {
        RawWaker::new(
            {
                let fut = unsafe { MyFuture::from_raw(p) };
                let new = fut.clone();
                std::mem::forget(fut);
                new.into_raw()
            },
            &RAW_WAKER_VTABLE,
        )
    },
    |p| {
        let fut = unsafe { MyFuture::from_raw(p) };
        get_rt().futures.push(fut);
    },
    |_| todo!(),
    |p| unsafe {
        MyFuture::from_raw(p);
    },
);

#[derive(Clone)]
struct MyFuture(Rc<UnsafeCell<Pin<Box<dyn Future<Output = ()>>>>>);

impl MyFuture {
    fn create_waker(&self) -> Waker {
        let data = Rc::into_raw(self.0.clone()) as *const ();
        let raw_waker = RawWaker::new(data, &RAW_WAKER_VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }

    fn into_raw(self) -> *const () {
        Rc::into_raw(self.0) as *const ()
    }

    unsafe fn from_raw(ptr: *const ()) -> Self {
        Self(Rc::from_raw(
            ptr as *const UnsafeCell<Pin<Box<dyn Future<Output = ()>>>>,
        ))
    }
}

#[cfg(test)]
mod tests {
    extern crate test;

    use std::{
        rc::Rc,
        sync::atomic::{AtomicU8, Ordering},
    };

    use crate::get_rt;

    pub fn with_runtime<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let rt = get_rt();
        let result = f();
        rt.clear();
        result
    }

    #[test]
    fn test_rt_simple_tick() {
        let counter = Rc::new(AtomicU8::new(0));

        {
            let counter = counter.clone();
            crate::spawn(async move {
                counter.fetch_add(1, Ordering::Relaxed);
            });
        }

        crate::tick();

        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[bench]
    fn bench_rt_empty(b: &mut test::Bencher) {
        with_runtime(|| {
            get_rt().clear();
            b.iter(|| crate::tick());
        });
    }

    #[bench]
    fn bench_rt_1_future(b: &mut test::Bencher) {
        with_runtime(|| {
            crate::spawn(async {
                loop {
                    crate::wait::next_tick().await;
                }
            });

            b.iter(|| crate::tick());
        });
    }

    #[bench]
    fn bench_rt_2_futures(b: &mut test::Bencher) {
        with_runtime(|| {
            for _ in 0..2 {
                crate::spawn(async {
                    loop {
                        crate::wait::next_tick().await;
                    }
                });
            }

            b.iter(|| crate::tick());
        });
    }

    #[bench]
    fn bench_rt_5_futures(b: &mut test::Bencher) {
        with_runtime(|| {
            for _ in 0..5 {
                crate::spawn(async {
                    loop {
                        crate::wait::next_tick().await;
                    }
                });
            }

            b.iter(|| crate::tick());
        });
    }

    #[bench]
    fn bench_rt_100_futures(b: &mut test::Bencher) {
        with_runtime(|| {
            for _ in 0..100 {
                crate::spawn(async {
                    loop {
                        crate::wait::next_tick().await;
                    }
                });
            }

            b.iter(|| crate::tick());
        });
    }

    #[bench]
    fn bench_rt_1000_futures(b: &mut test::Bencher) {
        with_runtime(|| {
            for _ in 0..1000 {
                crate::spawn(async {
                    loop {
                        crate::wait::next_tick().await;
                    }
                });
            }

            b.iter(|| crate::tick());
        });
    }

    #[bench]
    fn bench_rt_10_000_futures(b: &mut test::Bencher) {
        with_runtime(|| {
            for _ in 0..10_000 {
                crate::spawn(async {
                    loop {
                        crate::wait::next_tick().await;
                    }
                });
            }

            b.iter(|| crate::tick());
        });
    }
}
