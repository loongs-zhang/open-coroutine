use crate::blocker::Blocker;
use crate::coroutine::constants::{CoroutineState, Syscall, SyscallState};
use crate::coroutine::suspender::SimpleDelaySuspender;
use crate::coroutine::{Current, Named, StateMachine};
use crate::net::selector::{Selector, SelectorImpl};
use crate::pool::constants::PoolState;
use crate::pool::join::JoinHandle;
use crate::pool::task::TaskImpl;
use crate::pool::{CoroutinePool, CoroutinePoolImpl, Pool};
use crate::scheduler::{SchedulableCoroutine, SchedulableSuspender};
use polling::Event;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::fmt::Debug;
use std::io::{Error, ErrorKind};
use std::panic::RefUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[allow(trivial_numeric_casts, clippy::cast_possible_truncation)]
pub(crate) fn token() -> usize {
    if let Some(co) = SchedulableCoroutine::current() {
        #[allow(box_pointers)]
        let boxed: &'static mut CString = Box::leak(Box::from(
            CString::new(co.get_name()).expect("build name failed!"),
        ));
        let cstr: &'static CStr = boxed.as_c_str();
        cstr.as_ptr().cast::<c_void>() as usize
    } else {
        0
    }
}

pub trait EventLoop<'e>: Selector + Pool<'e, JoinHandleImpl<'e>> {
    fn wait_event(&self, timeout: Option<Duration>) -> std::io::Result<usize>;

    fn wait_just(&self, timeout: Option<Duration>) -> std::io::Result<usize>;
}

#[allow(missing_docs)]
#[repr(C)]
#[derive(Debug)]
pub struct JoinHandleImpl<'e>(*const EventLoopImpl<'e>, *const c_char);

impl<'e> JoinHandleImpl<'e> {
    #[allow(box_pointers)]
    pub(crate) fn new(event_loop: *const EventLoopImpl<'e>, name: &str) -> Self {
        let boxed: &'static mut CString = Box::leak(Box::from(
            CString::new(name).expect("init JoinHandle failed!"),
        ));
        let cstr: &'static CStr = boxed.as_c_str();
        JoinHandleImpl(event_loop, cstr.as_ptr())
    }

    /// create a error instance.
    #[must_use]
    pub fn error() -> Self {
        Self::new(std::ptr::null(), "")
    }
}

impl JoinHandle for JoinHandleImpl<'_> {
    fn get_name(&self) -> std::io::Result<&str> {
        unsafe { CStr::from_ptr(self.1) }
            .to_str()
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "Invalid task name"))
    }

    fn timeout_at_join(&self, timeout_time: u64) -> std::io::Result<Result<Option<usize>, &str>> {
        let name = self.get_name()?;
        if name.is_empty() {
            return Err(Error::new(ErrorKind::InvalidInput, "Invalid task name"));
        }
        let event_loop = unsafe { &*self.0 };
        event_loop
            .wait_result(
                name,
                Duration::from_nanos(timeout_time.saturating_sub(open_coroutine_timer::now())),
            )
            .map(|r| r.expect("result is None !").1)
    }
}

#[derive(Debug)]
pub struct EventLoopImpl<'e> {
    cpu: usize,
    pool: CoroutinePoolImpl<'e>,
    selector: SelectorImpl,
    stop: Arc<(Mutex<bool>, Condvar)>,
    shared_stop: Arc<(Mutex<AtomicUsize>, Condvar)>,
}

impl EventLoopImpl<'_> {
    pub fn new(
        name: String,
        cpu: usize,
        stack_size: usize,
        min_size: usize,
        max_size: usize,
        keep_alive_time: u64,
        shared_stop: Arc<(Mutex<AtomicUsize>, Condvar)>,
    ) -> std::io::Result<Self> {
        Ok(EventLoopImpl {
            cpu,
            pool: CoroutinePoolImpl::new(
                name,
                cpu,
                stack_size,
                min_size,
                max_size,
                keep_alive_time,
                crate::blocker::DelayBlocker::default(),
            ),
            selector: SelectorImpl::new()?,
            stop: Arc::new((Mutex::new(false), Condvar::new())),
            shared_stop,
        })
    }

    fn map_name<'c>(token: usize) -> Option<&'c str> {
        if token == 0 {
            return None;
        }
        unsafe { CStr::from_ptr((token as *const c_void).cast::<c_char>()) }
            .to_str()
            .ok()
    }
}

unsafe impl Send for EventLoopImpl<'_> {}

unsafe impl Sync for EventLoopImpl<'_> {}

impl RefUnwindSafe for EventLoopImpl<'_> {}

impl Default for EventLoopImpl<'_> {
    fn default() -> Self {
        Self::new(
            format!("open-coroutine-event-loop-{}", uuid::Uuid::new_v4()),
            1,
            crate::coroutine::DEFAULT_STACK_SIZE,
            0,
            65536,
            0,
            Arc::new((Mutex::new(AtomicUsize::new(0)), Condvar::new())),
        )
        .expect("create event-loop failed")
    }
}

impl Named for EventLoopImpl<'_> {
    fn get_name(&self) -> &str {
        self.pool.get_name()
    }
}

impl<'e> Pool<'e, JoinHandleImpl<'e>> for EventLoopImpl<'e> {
    fn get_state(&self) -> PoolState {
        self.pool.get_state()
    }

    fn change_state(&self, state: PoolState) -> PoolState {
        self.pool.change_state(state)
    }

    fn set_min_size(&self, min_size: usize) {
        self.pool.set_min_size(min_size);
    }

    fn get_min_size(&self) -> usize {
        self.pool.get_min_size()
    }

    fn get_running_size(&self) -> usize {
        self.pool.get_running_size()
    }

    fn set_max_size(&self, max_size: usize) {
        self.pool.set_max_size(max_size);
    }

    fn get_max_size(&self) -> usize {
        self.pool.get_max_size()
    }

    fn set_keep_alive_time(&self, keep_alive_time: u64) {
        self.pool.set_keep_alive_time(keep_alive_time);
    }

    fn get_keep_alive_time(&self) -> u64 {
        self.pool.get_keep_alive_time()
    }

    fn size(&self) -> usize {
        self.pool.size()
    }

    fn wait_result(
        &self,
        task_name: &str,
        wait_time: Duration,
    ) -> std::io::Result<Option<(String, Result<Option<usize>, &str>)>> {
        let mut left = wait_time;
        let once = Duration::from_millis(10);
        loop {
            if left.is_zero() {
                return Err(Error::new(ErrorKind::TimedOut, "wait timeout"));
            }
            if PoolState::Running == self.get_state() {
                //开启了单独的线程
                if let Ok(r) = self.pool.wait_result(task_name, left.min(once)) {
                    return Ok(r);
                }
            } else {
                _ = self.wait_event(Some(left.min(once)))?;
                if let Some(r) = self.pool.try_get_result(task_name) {
                    return Ok(Some(r));
                }
            }
            left = left.saturating_sub(once);
        }
    }

    fn submit_raw(&self, task: TaskImpl<'e>) -> JoinHandleImpl<'e> {
        let join_handle = self.pool.submit_raw(task);
        let task_name = join_handle.get_name().expect("Invalid task name");
        JoinHandleImpl::new(self, task_name)
    }

    fn pop(&self) -> Option<TaskImpl<'e>> {
        self.pool.pop()
    }

    #[allow(box_pointers)]
    fn change_blocker(&self, blocker: impl Blocker + 'e) -> Box<dyn Blocker>
    where
        'e: 'static,
    {
        self.pool.change_blocker(blocker)
    }

    fn start(self) -> std::io::Result<Arc<Self>>
    where
        'e: 'static,
    {
        assert_eq!(
            PoolState::Created,
            self.pool.change_state(PoolState::Running)
        );
        let arc = Arc::new(self);
        let consumer = arc.clone();
        let join_handle = std::thread::Builder::new()
            .name(arc.pool.get_name().to_string())
            .spawn(move || {
                // thread per core
                _ = core_affinity::set_for_current(core_affinity::CoreId { id: consumer.cpu });
                while PoolState::Running == consumer.get_state()
                    || !consumer.pool.is_empty()
                    || consumer.pool.get_running_size() > 0
                {
                    _ = consumer.wait_event(Some(Duration::from_millis(10)));
                }
                let (lock, cvar) = &*consumer.stop.clone();
                let mut pending = lock.lock().unwrap();
                *pending = false;
                cvar.notify_one();
                // notify shared stop flag
                let (lock, cvar) = &*consumer.shared_stop.clone();
                let pending = lock.lock().unwrap();
                _ = pending.fetch_add(1, Ordering::Release);
                cvar.notify_one();
                crate::warn!("{} has exited", consumer.get_name());
            })
            .map_err(|e| Error::new(ErrorKind::Other, format!("{e:?}")))?;
        std::mem::forget(join_handle);
        Ok(arc)
    }

    fn stop(&self, wait_time: Duration) -> std::io::Result<()> {
        let state = self.get_state();
        if PoolState::Stopped == state {
            return Ok(());
        }
        _ = self.pool.stop(Duration::ZERO);
        if PoolState::Running == state {
            //开启了单独的线程
            let (lock, cvar) = &*self.stop;
            let result = cvar
                .wait_timeout_while(lock.lock().unwrap(), wait_time, |&mut pending| pending)
                .unwrap();
            if result.1.timed_out() {
                return Err(Error::new(ErrorKind::TimedOut, "stop timeout !"));
            }
            _ = self.change_state(PoolState::Stopped);
            return Ok(());
        }
        let mut left = wait_time;
        let once = Duration::from_millis(10);
        loop {
            if left.is_zero() {
                return Err(Error::new(ErrorKind::TimedOut, "stop timeout !"));
            }
            _ = self.wait_event(Some(left.min(once)))?;
            if self.pool.is_empty() && self.pool.get_running_size() == 0 {
                _ = self.change_state(PoolState::Stopped);
                return Ok(());
            }
            left = left.saturating_sub(once);
        }
    }
}

impl Selector for EventLoopImpl<'_> {
    fn select(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> std::io::Result<usize> {
        self.selector.select(events, timeout)
    }

    fn add_read_event(&self, fd: c_int, token: usize) -> std::io::Result<()> {
        self.selector.add_read_event(fd, token)
    }

    fn add_write_event(&self, fd: c_int, token: usize) -> std::io::Result<()> {
        self.selector.add_write_event(fd, token)
    }

    fn del_event(&self, fd: c_int) -> std::io::Result<()> {
        self.selector.del_event(fd)
    }

    fn del_read_event(&self, fd: c_int) -> std::io::Result<()> {
        self.selector.del_read_event(fd)
    }

    fn del_write_event(&self, fd: c_int) -> std::io::Result<()> {
        self.selector.del_write_event(fd)
    }
}

impl<'e> EventLoop<'e> for EventLoopImpl<'e> {
    fn wait_event(&self, timeout: Option<Duration>) -> std::io::Result<usize> {
        if let Some(time) = timeout {
            if let Some(coroutine) = SchedulableCoroutine::current() {
                if let Some(suspender) = SchedulableSuspender::current() {
                    let timeout_time = open_coroutine_timer::get_timeout_time(time);
                    let syscall = match coroutine.state() {
                        CoroutineState::Running => {
                            cfg_if::cfg_if! {
                                if #[cfg(target_os = "linux")] {
                                    Syscall::epoll_wait
                                } else if #[cfg(any(
                                    target_os = "macos",
                                    target_os = "ios",
                                    target_os = "tvos",
                                    target_os = "watchos",
                                    target_os = "freebsd",
                                    target_os = "dragonfly",
                                    target_os = "openbsd",
                                    target_os = "netbsd"
                                ))] {
                                    Syscall::kevent
                                } else if #[cfg(windows)] {
                                    Syscall::iocp
                                }
                            }
                        }
                        CoroutineState::SystemCall((), syscall, _) => syscall,
                        _ => unreachable!("wait should never execute to here"),
                    };
                    coroutine
                        .syscall((), syscall, SyscallState::Suspend(timeout_time))
                        .expect("change to syscall state failed !");
                    //协程环境只yield，不执行任务
                    suspender.delay(time);
                    return self.wait_just(Some(Duration::ZERO));
                }
            }
            return self.wait_just(Some(
                self.pool
                    .try_timed_schedule(time)
                    .map(Duration::from_nanos)?,
            ));
        }
        self.pool.try_schedule()?;
        self.wait_just(None)
    }

    fn wait_just(&self, timeout: Option<Duration>) -> std::io::Result<usize> {
        cfg_if::cfg_if! {
            if #[cfg(target_os = "linux")] {
                let net_syscall = Syscall::epoll_wait;
            } else if #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "watchos",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd"
            ))] {
                let net_syscall = Syscall::kevent;
            } else if #[cfg(windows)] {
                let net_syscall = Syscall::iocp;
            }
        }
        if let Some(coroutine) = SchedulableCoroutine::current() {
            match coroutine.state() {
                CoroutineState::SystemCall(
                    (),
                    syscall,
                    SyscallState::Computing | SyscallState::Timeout,
                ) => {
                    coroutine
                        .syscall((), syscall, SyscallState::Calling(net_syscall))
                        .expect("change to syscall state failed !");
                }
                _ => unreachable!("wait should never execute to here"),
            };
        }
        let mut events = Vec::with_capacity(1024);
        let result = self.selector.select(&mut events, timeout);
        if let Some(coroutine) = SchedulableCoroutine::current() {
            match coroutine.state() {
                CoroutineState::SystemCall((), syscall, SyscallState::Calling(_)) => {
                    coroutine
                        .syscall((), syscall, SyscallState::Computing)
                        .expect("change to syscall state failed !");
                }
                _ => unreachable!("wait should never execute to here"),
            };
        }
        let count = result?;
        if count > 0 {
            //遍历events
            for event in &events {
                if let Some(co_name) = Self::map_name(event.key) {
                    //notify coroutine
                    self.pool.try_resume(co_name).expect("has bug, notice !");
                }
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_simple() -> std::io::Result<()> {
        let event_loop = EventLoopImpl::default();
        event_loop.set_max_size(1);
        _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
        _ = event_loop.submit(
            None,
            |_| {
                println!("2");
                Some(2)
            },
            None,
        );
        event_loop.stop(Duration::from_secs(3))
    }

    #[test]
    fn test_wait() -> std::io::Result<()> {
        let event_loop = EventLoopImpl::default();
        event_loop.set_max_size(1);
        let task_name = uuid::Uuid::new_v4().to_string();
        _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
        let result = event_loop.submit_and_wait(
            Some(task_name.clone()),
            |_| {
                println!("2");
                Some(2)
            },
            None,
            Duration::from_millis(100),
        );
        assert_eq!(Some((task_name, Ok(Some(2)))), result.unwrap());
        event_loop.stop(Duration::from_secs(3))
    }

    #[test]
    fn test_simple_auto() -> std::io::Result<()> {
        let event_loop = EventLoopImpl::default().start()?;
        event_loop.set_max_size(1);
        _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
        _ = event_loop.submit(
            None,
            |_| {
                println!("2");
                Some(2)
            },
            None,
        );
        event_loop.stop(Duration::from_secs(3))
    }

    #[test]
    fn test_wait_auto() -> std::io::Result<()> {
        let event_loop = EventLoopImpl::default().start()?;
        event_loop.set_max_size(1);
        let task_name = uuid::Uuid::new_v4().to_string();
        _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
        let result = event_loop.submit_and_wait(
            Some(task_name.clone()),
            |_| {
                println!("2");
                Some(2)
            },
            None,
            Duration::from_secs(3),
        );
        assert_eq!(Some((task_name, Ok(Some(2)))), result.unwrap());
        event_loop.stop(Duration::from_secs(3))
    }
}
