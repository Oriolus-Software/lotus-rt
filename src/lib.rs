use std::{
    cell::UnsafeCell,
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
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

#[cfg(feature = "macros")]
pub use tokio::{join, pin, select, try_join};

#[derive(Default)]
struct Runtime {
    current_tick: u64,
    futures: VecDeque<MyFuture>,
    on_tick: BinaryHeap<Reverse<TickTimer>>,
}

impl Runtime {
    fn add_timer(&mut self, timer: TickTimer) {
        self.on_tick.push(Reverse(timer));
    }

    #[cfg(test)]
    fn clear(&mut self) {
        *self = Default::default();
    }
}

thread_local! {
    static RT: std::cell::UnsafeCell<Runtime> = std::cell::UnsafeCell::new(Runtime::default());
}

#[inline(always)]
fn get_rt() -> &'static mut Runtime {
    RT.with(|rt| unsafe { &mut *rt.get() })
}

pub struct JoinHandle<T>(crate::sync::oneshot::Receiver<T>);

#[derive(Debug, thiserror::Error)]
pub enum TryRecvError {
    #[error("the result has not been computed yet")]
    Empty,
    #[error("the future has been dropped")]
    FutureDropped,
}

impl From<tokio::sync::oneshot::error::TryRecvError> for TryRecvError {
    fn from(value: tokio::sync::oneshot::error::TryRecvError) -> Self {
        match value {
            tokio::sync::oneshot::error::TryRecvError::Empty => TryRecvError::Empty,
            tokio::sync::oneshot::error::TryRecvError::Closed => TryRecvError::FutureDropped,
        }
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for TryRecvError {
    fn from(_: tokio::sync::oneshot::error::RecvError) -> Self {
        TryRecvError::FutureDropped
    }
}

impl<T> JoinHandle<T> {
    #[inline(always)]
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        Ok(self.0.try_recv()?)
    }

    #[inline(always)]
    pub async fn into_future(self) -> Result<T, TryRecvError> {
        Ok(self.0.await?)
    }
}

pub fn spawn<F, R>(f: F) -> JoinHandle<R>
where
    F: Future<Output = R> + 'static,
    R: 'static,
{
    let (tx, rx) = crate::sync::oneshot::channel();
    get_rt().spawn(async move {
        let result = f.await;
        tx.send(result).ok();
    });

    JoinHandle(rx)
}

#[inline(always)]
pub fn tick() {
    get_rt().tick();
}

impl Runtime {
    #[inline(always)]
    pub fn spawn<F>(&mut self, f: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.futures
            .push_back(MyFuture(Rc::new(UnsafeCell::new(Box::pin(f)))));
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
            if unsafe { *timer.0.dropped.get() } {
                continue;
            }

            if timer.0.expires <= self.current_tick {
                (timer.0.callback)();
            } else {
                self.on_tick.push(timer);
                break;
            }
        }

        self.execute();
    }

    pub fn execute(&mut self) {
        while let Some(future) = self.futures.pop_front() {
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
    dropped: Rc<UnsafeCell<bool>>,
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
        get_rt().futures.push_back(fut);
    },
    |p| {
        let fut = unsafe { MyFuture::from_raw(p) };
        get_rt().futures.push_back(fut.clone());
        std::mem::forget(fut);
    },
    |p| unsafe {
        MyFuture::from_raw(p);
    },
);

#[derive(Clone)]
struct MyFuture(Rc<UnsafeCell<Pin<Box<dyn Future<Output = ()>>>>>);

impl MyFuture {
    fn create_waker(&self) -> Waker {
        let data = Rc::into_raw(self.0.clone()).cast();
        let raw_waker = RawWaker::new(data, &RAW_WAKER_VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }

    #[inline(always)]
    fn into_raw(self) -> *const () {
        Rc::into_raw(self.0) as *const ()
    }

    #[inline(always)]
    unsafe fn from_raw(ptr: *const ()) -> Self {
        Self(Rc::from_raw(ptr.cast()))
    }
}

#[cfg(test)]
mod tests {
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
        rt.clear();
        let result = f();
        rt.clear();
        result
    }

    #[test]
    fn test_rt_simple_tick() {
        with_runtime(|| {
            let counter = Rc::new(AtomicU8::new(0));

            {
                let counter = counter.clone();
                crate::spawn(async move {
                    counter.fetch_add(1, Ordering::Relaxed);
                });
            }

            crate::tick();

            assert_eq!(counter.load(Ordering::Relaxed), 1);
        });
    }
}
