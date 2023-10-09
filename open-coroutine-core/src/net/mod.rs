use crate::monitor::Monitor;
use crate::net::config::Config;
use crate::net::event_loop::join::JoinHandleImpl;
use crate::net::event_loop::{EventLoop, EventLoopImpl};
use crate::net::selector::Selector;
use crate::pool::task::TaskImpl;
use crate::pool::{CoroutinePool, Pool};
use once_cell::sync::{Lazy, OnceCell};
use std::ffi::c_int;
use std::panic::UnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Event driven abstraction and impl.
pub mod selector;

#[allow(missing_docs, clippy::missing_errors_doc)]
pub mod event_loop;

#[allow(missing_docs)]
pub mod config;

#[derive(Debug, Copy, Clone)]
pub struct EventLoops {}

static INDEX: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));

static EVENT_LOOPS: Lazy<Box<[EventLoopImpl]>> = Lazy::new(|| {
    let config = Config::get_instance();
    (0..config.get_event_loop_size())
        .map(|i| {
            EventLoopImpl::new(
                format!("open-coroutine-event-loop-{i}"),
                i,
                config.get_stack_size(),
                config.get_min_size(),
                config.get_max_size(),
                config.get_keep_alive_time(),
                crate::blocker::DelayBlocker::default(),
            )
            .unwrap_or_else(|_| panic!("init event-loop-{i} failed!"))
        })
        .collect()
});

static EVENT_LOOP_WORKERS: OnceCell<Box<[std::thread::JoinHandle<()>]>> = OnceCell::new();

static EVENT_LOOP_STARTED: Lazy<AtomicBool> = Lazy::new(AtomicBool::default);

static EVENT_LOOP_STOP: Lazy<Arc<(Mutex<AtomicUsize>, Condvar)>> =
    Lazy::new(|| Arc::new((Mutex::new(AtomicUsize::new(0)), Condvar::new())));

impl EventLoops {
    fn next(skip_monitor: bool) -> &'static EventLoopImpl<'static> {
        let mut index = INDEX.fetch_add(1, Ordering::SeqCst);
        if skip_monitor && index % EVENT_LOOPS.len() == 0 {
            INDEX.store(1, Ordering::SeqCst);
            EVENT_LOOPS.get(1).expect("init event-loop-1 failed!")
        } else {
            index %= EVENT_LOOPS.len();
            EVENT_LOOPS
                .get(index)
                .unwrap_or_else(|| panic!("init event-loop-{index} failed!"))
        }
    }

    pub(crate) fn new_condition() -> Arc<(Mutex<AtomicUsize>, Condvar)> {
        Arc::clone(&EVENT_LOOP_STOP)
    }

    fn start() {
        if EVENT_LOOP_STARTED
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            cfg_if::cfg_if! {
                if #[cfg(all(unix, feature = "preemptive-schedule"))] {
                    _ = crate::monitor::MonitorImpl::change_blocker(crate::blocker::NetBlocker(
                        EVENT_LOOPS.get(1).expect("init event-loop-0 failed!"),
                    ));
                }
            }
            //初始化event_loop线程
            _ = EVENT_LOOP_WORKERS.get_or_init(|| {
                (1..EVENT_LOOPS.len())
                    .map(|i| {
                        std::thread::Builder::new()
                            .name(format!("open-coroutine-event-loop-{i}"))
                            .spawn(move || {
                                assert!(
                                    core_affinity::set_for_current(core_affinity::CoreId { id: i }),
                                    "pin event loop thread to a single CPU core failed !"
                                );
                                let token = EventLoopImpl::token();
                                let event_loop = EventLoops::next(true);
                                while EVENT_LOOP_STARTED.load(Ordering::Acquire)
                                    || !event_loop.is_empty()
                                {
                                    _ = event_loop
                                        .wait_event(token, Some(Duration::from_millis(10)));
                                }
                                crate::warn!("open-coroutine-event-loop-{i} has exited");
                                let pair = EventLoops::new_condition();
                                let (lock, cvar) = pair.as_ref();
                                let pending = lock.lock().unwrap();
                                _ = pending.fetch_add(1, Ordering::Release);
                                cvar.notify_one();
                            })
                            .expect("failed to spawn event-loop thread")
                    })
                    .collect()
            });
        }
    }

    pub fn stop() {
        crate::warn!("open-coroutine is exiting...");
        #[cfg(all(unix, feature = "preemptive-schedule"))]
        crate::monitor::MonitorImpl::get_instance().stop();
        EVENT_LOOP_STARTED.store(false, Ordering::Release);
        // wait for the event-loops to stop
        let (lock, cvar) = EVENT_LOOP_STOP.as_ref();
        let result = cvar
            .wait_timeout_while(
                lock.lock().unwrap(),
                Duration::from_millis(30000),
                |stopped| {
                    cfg_if::cfg_if! {
                        if #[cfg(all(unix, feature = "preemptive-schedule"))] {
                            let condition = EVENT_LOOPS.len();
                        } else {
                            let condition = EVENT_LOOPS.len() - 1;
                        }
                    }
                    stopped.load(Ordering::Acquire) < condition
                },
            )
            .unwrap()
            .1;
        if result.timed_out() {
            crate::error!("open-coroutine didn't exit successfully within 30 seconds !");
        } else {
            crate::info!("open-coroutine exit successfully !");
        }
    }

    pub fn submit(
        f: impl FnOnce(Option<usize>) -> Option<usize> + UnwindSafe + 'static,
        param: Option<usize>,
    ) -> JoinHandleImpl<'static> {
        EventLoops::start();
        EventLoops::next(true).submit(None, f, param)
    }

    pub(crate) fn submit_raw(task: TaskImpl<'static>) {
        _ = EventLoops::next(true).submit_raw(task);
    }

    pub fn wait_event(timeout: Option<Duration>) -> std::io::Result<()> {
        EventLoops::next(false).wait_just(timeout)
    }

    pub fn wait_read_event(fd: c_int, timeout: Option<Duration>) -> std::io::Result<()> {
        EventLoops::next(false).wait_read(fd, timeout)
    }

    pub fn wait_write_event(fd: c_int, timeout: Option<Duration>) -> std::io::Result<()> {
        EventLoops::next(false).wait_write(fd, timeout)
    }

    pub fn del_event(fd: c_int) {
        (0..EVENT_LOOPS.len()).for_each(|_| {
            _ = EventLoops::next(false).del_event(fd);
        });
    }

    pub fn del_read_event(fd: c_int) {
        (0..EVENT_LOOPS.len()).for_each(|_| {
            _ = EventLoops::next(false).del_read_event(fd);
        });
    }

    pub fn del_write_event(fd: c_int) {
        (0..EVENT_LOOPS.len()).for_each(|_| {
            _ = EventLoops::next(false).del_write_event(fd);
        });
    }
}
