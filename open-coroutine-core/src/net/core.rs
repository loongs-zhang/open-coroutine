#[cfg(all(unix, feature = "preemptive-schedule"))]
use crate::monitor::Monitor;
use crate::net::config::Config;
use crate::net::event_loop::{EventLoop, EventLoopImpl, JoinHandleImpl};
use crate::net::selector::Selector;
#[cfg(all(unix, feature = "preemptive-schedule"))]
use crate::pool::task::TaskImpl;
use crate::pool::Pool;
use once_cell::sync::Lazy;
use std::ffi::c_int;
use std::panic::UnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug, Copy, Clone)]
pub struct EventLoops {}

static INDEX: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));

static EVENT_LOOP_STOP: Lazy<Arc<(Mutex<AtomicUsize>, Condvar)>> =
    Lazy::new(|| Arc::new((Mutex::new(AtomicUsize::new(0)), Condvar::new())));

static EVENT_LOOPS: Lazy<Box<[Arc<EventLoopImpl>]>> = Lazy::new(|| {
    let config = Config::get_instance();
    (0..config.get_event_loop_size())
        .map(|i| {
            let event_loop = EventLoopImpl::new(
                format!("open-coroutine-event-loop-{i}"),
                i,
                config.get_stack_size(),
                config.get_min_size(),
                config.get_max_size(),
                config.get_keep_alive_time(),
                EVENT_LOOP_STOP.clone(),
            )
            .unwrap_or_else(|_| panic!("init event-loop-{i} failed!"));
            cfg_if::cfg_if! {
                if #[cfg(all(unix, feature = "preemptive-schedule"))] {
                    if i == 0 {
                        return Arc::new(event_loop);
                    }
                }
            }
            event_loop
                .start()
                .unwrap_or_else(|_| panic!("init event-loop-{i} failed!"))
        })
        .collect()
});

impl EventLoops {
    #[allow(unused_variables)]
    fn next(skip_monitor: bool) -> (&'static Arc<EventLoopImpl<'static>>, bool) {
        cfg_if::cfg_if! {
            if #[cfg(not(all(unix, feature = "preemptive-schedule")))] {
                let skip_monitor = false;
            }
        }
        let mut index = INDEX.fetch_add(1, Ordering::Release);
        if skip_monitor && index % EVENT_LOOPS.len() == crate::MONITOR_CPU {
            INDEX.store(1, Ordering::Release);
            (
                EVENT_LOOPS.get(1).expect("init event-loop-1 failed!"),
                false,
            )
        } else {
            index %= EVENT_LOOPS.len();
            cfg_if::cfg_if! {
                if #[cfg(all(unix, feature = "preemptive-schedule"))] {
                    let is_monitor = crate::MONITOR_CPU == index;
                } else {
                    let is_monitor = false;
                }
            }
            (
                EVENT_LOOPS
                    .get(index)
                    .unwrap_or_else(|| panic!("init event-loop-{index} failed!")),
                is_monitor,
            )
        }
    }

    #[cfg(all(unix, feature = "preemptive-schedule"))]
    pub(crate) fn monitor() -> &'static Arc<EventLoopImpl<'static>> {
        //monitor线程的EventLoop固定
        EVENT_LOOPS
            .get(crate::MONITOR_CPU)
            .expect("init event-loop-monitor failed!")
    }

    pub fn stop() {
        crate::warn!("open-coroutine is exiting...");
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next(true).0.stop(Duration::ZERO);
        }
        // wait for the event-loops to stop
        let (lock, cvar) = &**EVENT_LOOP_STOP;
        let result = cvar
            .wait_timeout_while(
                lock.lock().unwrap(),
                Duration::from_millis(30000),
                |stopped| {
                    cfg_if::cfg_if! {
                        if #[cfg(all(unix, feature = "preemptive-schedule"))] {
                            let condition = EVENT_LOOPS.len() - 1;
                        } else {
                            let condition = EVENT_LOOPS.len();
                        }
                    }
                    stopped.load(Ordering::Acquire) < condition
                },
            )
            .unwrap()
            .1;
        #[cfg(all(unix, feature = "preemptive-schedule"))]
        crate::monitor::MonitorImpl::get_instance().stop();
        if result.timed_out() {
            crate::error!("open-coroutine didn't exit successfully within 30 seconds !");
        } else {
            crate::info!("open-coroutine exit successfully !");
        }
    }

    pub fn submit(
        name: Option<String>,
        f: impl FnOnce(Option<usize>) -> Option<usize> + UnwindSafe + 'static,
        param: Option<usize>,
    ) -> JoinHandleImpl<'static> {
        EventLoops::next(true).0.submit(name, f, param)
    }

    #[cfg(all(unix, feature = "preemptive-schedule"))]
    pub(crate) fn submit_raw(task: TaskImpl<'static>) {
        _ = EventLoops::next(true).0.submit_raw(task);
    }

    pub fn wait_event(timeout: Option<Duration>) -> std::io::Result<usize> {
        EventLoops::next(true).0.wait_event(timeout)
    }

    pub fn wait_read_event(
        fd: c_int,
        added: &AtomicBool,
        timeout: Option<Duration>,
    ) -> std::io::Result<usize> {
        let (event_loop, is_monitor) = EventLoops::next(false);
        if added
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            event_loop.add_read_event(fd, crate::net::event_loop::token())?;
        }
        if is_monitor {
            Self::wait_event(timeout)
        } else {
            event_loop.wait_event(timeout)
        }
    }

    pub fn wait_write_event(
        fd: c_int,
        added: &AtomicBool,
        timeout: Option<Duration>,
    ) -> std::io::Result<usize> {
        let (event_loop, is_monitor) = EventLoops::next(false);
        if added
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            event_loop.add_write_event(fd, crate::net::event_loop::token())?;
        }
        if is_monitor {
            Self::wait_event(timeout)
        } else {
            event_loop.wait_event(timeout)
        }
    }

    pub fn del_event(fd: c_int) {
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next(false).0.del_event(fd);
        }
    }

    pub fn del_read_event(fd: c_int) {
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next(false).0.del_read_event(fd);
        }
    }

    pub fn del_write_event(fd: c_int) {
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next(false).0.del_write_event(fd);
        }
    }
}
