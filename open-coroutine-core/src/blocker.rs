use crate::coroutine::suspender::SimpleDelaySuspender;
use crate::coroutine::{Current, Named};
use crate::scheduler::SchedulableSuspender;
use std::fmt::Debug;
use std::time::Duration;

/// A trait for blocking current thread.
pub trait Blocker: Debug + Named {
    /// Block current thread for a while.
    fn block(&self, dur: Duration);
}

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct DelayBlocker {}

/// const `DELAY_BLOCKER_NAME`.
pub const DELAY_BLOCKER_NAME: &str = "DelayBlocker";

impl Named for DelayBlocker {
    fn get_name(&self) -> &str {
        DELAY_BLOCKER_NAME
    }
}

impl Blocker for DelayBlocker {
    fn block(&self, dur: Duration) {
        if let Some(suspender) = SchedulableSuspender::current() {
            suspender.delay(dur);
        }
    }
}

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Default)]
pub struct SleepBlocker {}

/// const `SLEEP_BLOCKER_NAME`.
pub const SLEEP_BLOCKER_NAME: &str = "SleepBlocker";

impl Named for SleepBlocker {
    fn get_name(&self) -> &str {
        SLEEP_BLOCKER_NAME
    }
}

impl Blocker for SleepBlocker {
    fn block(&self, dur: Duration) {
        std::thread::sleep(dur);
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "net")] {
        use crate::net::event_loop::EventLoop;
        use crate::pool::{CoroutinePool, Pool};

        #[allow(missing_docs)]
        #[derive(Debug, Copy, Clone)]
        pub struct NetBlocker(pub &'static crate::net::event_loop::EventLoopImpl<'static>);

        /// const `NET_BLOCKER_NAME`.
        pub const NET_BLOCKER_NAME: &str = "NetBlocker";

        impl Named for NetBlocker {
            fn get_name(&self) -> &str {
                NET_BLOCKER_NAME
            }
        }

        impl Blocker for NetBlocker {
            fn block(&self, dur: Duration) {
                _ = self.0.wait_just(Some(dur));
                while !self.0.is_empty() {
                    if let Some(task) = self.0.pop() {
                        crate::net::EventLoops::submit_raw(task);
                    }
                }
            }
        }
    }
}
