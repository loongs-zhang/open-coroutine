use crate::constants::MONITOR_CPU;
#[cfg(all(unix, feature = "preemptive-schedule"))]
use crate::monitor::Monitor;
use crate::net::config::Config;
use crate::net::event_loop::core::{EventLoop, EventLoopImpl};
use crate::net::event_loop::join::JoinHandleImpl;
use crate::net::selector::Selector;
use crate::pool::{AutoConsumableTaskPool, SubmittableTaskPool};
use once_cell::sync::Lazy;
use std::ffi::c_int;
use std::panic::UnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub mod join;

pub mod core;

#[cfg(test)]
mod tests;

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
            event_loop
                .start()
                .unwrap_or_else(|_| panic!("init event-loop-{i} failed!"))
        })
        .collect()
});

impl EventLoops {
    fn next() -> &'static Arc<EventLoopImpl<'static>> {
        let index = INDEX.fetch_add(1, Ordering::Release);
        if index % EVENT_LOOPS.len() == MONITOR_CPU {
            INDEX.store(0, Ordering::Release);
        }
        EVENT_LOOPS
            .get(index)
            .unwrap_or_else(|| panic!("init event-loop-{index} failed!"))
    }

    pub fn stop() {
        crate::warn!("open-coroutine is exiting...");
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next().stop(Duration::ZERO);
        }
        // wait for the event-loops to stop
        let (lock, cvar) = &**EVENT_LOOP_STOP;
        let result = cvar
            .wait_timeout_while(
                lock.lock().unwrap(),
                Duration::from_millis(30000),
                |stopped| stopped.load(Ordering::Acquire) < EVENT_LOOPS.len(),
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
        EventLoops::next().submit(name, f, param)
    }

    pub fn wait_event(timeout: Option<Duration>) -> std::io::Result<()> {
        EventLoops::next().wait_event(timeout)
    }

    pub fn wait_read_event(
        fd: c_int,
        added: &AtomicBool,
        timeout: Option<Duration>,
    ) -> std::io::Result<()> {
        let event_loop = EventLoops::next();
        if added
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            event_loop.add_read(fd)?;
        }
        event_loop.wait_event(timeout)
    }

    pub fn wait_write_event(
        fd: c_int,
        added: &AtomicBool,
        timeout: Option<Duration>,
    ) -> std::io::Result<()> {
        let event_loop = EventLoops::next();
        if added
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            event_loop.add_write(fd)?;
        }
        event_loop.wait_event(timeout)
    }

    pub fn del_event(fd: c_int) {
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next().del_event(fd);
        }
    }

    pub fn del_read_event(fd: c_int) {
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next().del_read_event(fd);
        }
    }

    pub fn del_write_event(fd: c_int) {
        for _ in 0..EVENT_LOOPS.len() {
            _ = EventLoops::next().del_write_event(fd);
        }
    }
}
