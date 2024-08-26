use std::{
    cell::UnsafeCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{RawWaker, RawWakerVTable, Waker},
};

#[derive(Default)]
pub struct Runtime {
    todo: Vec<Pin<Box<dyn Future<Output = ()>>>>,
    pending: Vec<Pin<Box<dyn Future<Output = ()>>>>,
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
        self.todo.push(Box::pin(f));
    }

    pub fn tick(&mut self) {
        for f in self.on_tick.drain(..) {
            f();
        }

        let mut pending = Vec::with_capacity(self.todo.len());

        while let Some(mut future) = self.todo.pop() {
            let raw_waker = RawWaker::new(std::ptr::null(), &RAW_WAKER_VTABLE);
            let waker = unsafe { Waker::from_raw(raw_waker) };

            let mut cx = std::task::Context::from_waker(&waker);
            match future.as_mut().poll(&mut cx) {
                std::task::Poll::Ready(_) => {}
                std::task::Poll::Pending => pending.push(future),
            }
        }

        self.pending.extend(pending);
    }
}

const RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |p| RawWaker::new(p, &RAW_WAKER_VTABLE),
    |_| todo!(),
    |_| todo!(),
    |_| {},
);

struct MyFuture {
    fut: Pin<Box<dyn Future<Output = ()>>>,
    fut_data: Rc<UnsafeCell<MyFutureData>>,
}

struct MyFutureData {
    should_poll: bool,
}

struct NextTickFut {
    polled: bool,
}

impl Future for NextTickFut {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if !self.polled {
            let waker = cx.waker().clone();
            let b = &mut self.polled as *mut bool;
            get_rt().on_tick.push(Box::new(move || {
                unsafe { *b = true };
                waker.wake();
            }));
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    }
}

pub fn next_tick() -> impl Future<Output = ()> {
    NextTickFut { polled: false }
}
