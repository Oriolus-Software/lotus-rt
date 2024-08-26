pub use self::ticks::*;
mod ticks {
    use std::{future::Future, ops::DerefMut, pin::Pin, task::Poll};

    use crate::get_rt;

    #[derive(Clone, Copy)]
    enum WaitTicks {
        Created(u64),
        Waiting,
        Done,
    }

    impl Future for WaitTicks {
        type Output = ();

        fn poll(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            match self.clone() {
                WaitTicks::Created(to_wait) => {
                    let waker = cx.waker().clone();
                    let ptr = self.deref_mut() as *mut Self;

                    *self = WaitTicks::Waiting;

                    crate::get_rt().on_tick.push(crate::TickTimer {
                        expires: get_rt().current_tick + to_wait,
                        callback: Box::new(move || {
                            unsafe { *ptr = WaitTicks::Done };
                            waker.wake();
                        }),
                    });

                    Poll::Pending
                }
                WaitTicks::Waiting => Poll::Pending,
                WaitTicks::Done => Poll::Ready(()),
            }
        }
    }

    pub fn next_tick() -> impl Future<Output = ()> {
        WaitTicks::Created(1)
    }

    pub fn ticks(count: u64) -> impl Future<Output = ()> {
        WaitTicks::Created(count)
    }
}

pub async fn seconds(count: f32) {
    let mut elapsed = 0.0;
    #[cfg(feature = "std")]
    let start = std::time::Instant::now();

    while elapsed < count {
        next_tick().await;
        #[cfg(feature = "std")]
        {
            elapsed = start.elapsed().as_secs_f32();
        }

        #[cfg(not(feature = "std"))]
        {
            elapsed += lotus_script::delta();
        }
    }
}

#[cfg(feature = "lotus")]
pub use self::lotus::*;
#[cfg(feature = "lotus")]
mod lotus {
    use lotus_script::input::{ActionState, ActionStateKind};

    pub async fn action(id: &str) -> ActionState {
        loop {
            let state = lotus_script::action::state(id);
            if state.kind != ActionStateKind::None {
                return state;
            }

            super::next_tick().await;
        }
    }

    pub async fn just_pressed(id: &str) -> ActionState {
        loop {
            let state = self::action(id).await;
            if state.kind.is_just_pressed() {
                return state;
            }
        }
    }

    pub async fn just_released(id: &str) -> ActionState {
        loop {
            let state = self::action(id).await;
            if state.kind.is_just_released() {
                return state;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        rc::Rc,
        sync::atomic::{AtomicU8, Ordering},
    };

    #[test]
    fn test_next_tick() {
        let counter = Rc::new(AtomicU8::new(0));

        {
            let counter = counter.clone();
            crate::spawn(async move {
                crate::wait::next_tick().await;
                counter.fetch_add(1, Ordering::SeqCst);
            });
        }

        crate::tick();

        assert_eq!(counter.load(Ordering::SeqCst), 0);

        crate::tick();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
