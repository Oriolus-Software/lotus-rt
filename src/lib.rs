use std::{
    cell::UnsafeCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{RawWaker, RawWakerVTable, Waker},
};

#[derive(Default)]
struct Runtime {
    futures: Vec<MyFuture>,
    on_tick: Vec<Box<dyn FnOnce()>>,
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
        for f in self.on_tick.drain(..) {
            f();
        }

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

pub mod wait {
    use std::{future::Future, pin::Pin};

    use crate::get_rt;

    struct WaitTicks {
        left: usize,
    }

    impl Future for WaitTicks {
        type Output = ();

        fn poll(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            if self.left != 0 {
                self.left -= 1;

                let waker = cx.waker().clone();
                get_rt().on_tick.push(Box::new(move || {
                    waker.wake();
                }));

                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        }
    }

    pub fn next_tick() -> impl Future<Output = ()> {
        WaitTicks { left: 1 }
    }

    pub fn ticks(count: usize) -> impl Future<Output = ()> {
        WaitTicks { left: count }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_next_tick() {}
    }
}
