pub use self::ticks::*;
mod ticks {
    use std::{
        cell::UnsafeCell,
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll},
    };

    use crate::get_rt;

    enum WaitTicks {
        Created(u64),
        Waiting {
            dropped: Rc<UnsafeCell<bool>>,
            completed: Rc<UnsafeCell<bool>>,
        },
        Done,
    }

    impl Future for WaitTicks {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            match &*self {
                WaitTicks::Created(to_wait) => {
                    let waker = cx.waker().clone();
                    let dropped = Rc::new(UnsafeCell::new(false));
                    let completed = Rc::new(UnsafeCell::new(false));

                    {
                        let dropped = dropped.clone();
                        let completed = completed.clone();
                        crate::get_rt().add_timer(crate::TickTimer {
                            expires: get_rt().current_tick + to_wait,
                            dropped,
                            callback: Box::new(move || {
                                unsafe { *completed.get() = true };
                                waker.wake();
                            }),
                        });
                    }

                    *self = WaitTicks::Waiting { dropped, completed };
                    Poll::Pending
                }
                WaitTicks::Waiting { completed, .. } => {
                    if unsafe { *completed.get() } {
                        *self = WaitTicks::Done;
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                }
                WaitTicks::Done => Poll::Ready(()),
            }
        }
    }

    impl Drop for WaitTicks {
        #[inline(always)]
        fn drop(&mut self) {
            if let WaitTicks::Waiting { dropped, .. } = self {
                unsafe { *dropped.get() = true };
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
            elapsed += lotus_script::time::delta();
        }
    }
}

#[cfg(feature = "lotus")]
pub use self::lotus::*;
#[cfg(feature = "lotus")]
mod lotus {
    use lotus_script::{
        input::{ActionState, ActionStateKind},
        var::VariableType,
    };

    pub async fn action(id: &str) -> ActionState {
        loop {
            super::next_tick().await;

            let state = lotus_script::action::state(id);
            if state.kind != ActionStateKind::None {
                return state;
            }
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

    pub async fn variable_change<T: VariableType<Output: PartialEq>>(name: &str) -> T::Output {
        let current = T::get_var(name);
        loop {
            super::next_tick().await;

            let new = T::get_var(name);
            if new != current {
                return new;
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

    use crate::tests::with_runtime;

    #[test]
    fn test_next_tick() {
        with_runtime(|| {
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
        });
    }
}
