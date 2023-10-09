use crate::blocker::Blocker;
use crate::coroutine::constants::{CoroutineState, Syscall, SyscallState};
use crate::coroutine::suspender::SimpleDelaySuspender;
use crate::coroutine::{Current, Named, StateMachine};
use crate::net::event_loop::join::JoinHandleImpl;
use crate::net::selector::{Selector, SelectorImpl};
use crate::pool::constants::PoolState;
use crate::pool::join::JoinHandle;
use crate::pool::task::TaskImpl;
use crate::pool::{CoroutinePool, CoroutinePoolImpl, Pool};
use crate::scheduler::{SchedulableCoroutine, SchedulableSuspender};
use dashmap::DashMap;
use polling::Events;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::fmt::Debug;
use std::io::{Error, ErrorKind};
use std::panic::RefUnwindSafe;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub mod join;

pub trait EventLoop<'e>: Selector + CoroutinePool<'e, JoinHandleImpl<'e>> {
    #[allow(trivial_numeric_casts, clippy::cast_possible_truncation)]
    #[must_use]
    fn token() -> usize {
        if let Some(co) = SchedulableCoroutine::current() {
            #[allow(box_pointers)]
            let boxed: &'static mut CString = Box::leak(Box::from(
                CString::new(co.get_name()).expect("build name failed!"),
            ));
            let cstr: &'static CStr = boxed.as_c_str();
            cstr.as_ptr().cast::<c_void>() as usize
        } else {
            unsafe {
                cfg_if::cfg_if! {
                    if #[cfg(windows)] {
                        windows_sys::Win32::System::Threading::GetCurrentThread() as usize
                    } else {
                        libc::pthread_self() as usize
                    }
                }
            }
        }
    }

    fn wait_write(&self, socket: c_int, dur: Option<Duration>) -> std::io::Result<()> {
        let token = Self::token();
        self.add_write_event(socket, token)?;
        self.wait_event(token, dur)
    }

    fn wait_read(&self, socket: c_int, dur: Option<Duration>) -> std::io::Result<()> {
        let token = Self::token();
        self.add_read_event(socket, token)?;
        self.wait_event(token, dur)
    }

    fn wait_event(&self, token: usize, timeout: Option<Duration>) -> std::io::Result<()> {
        self.wait(token, timeout, true)
    }

    fn wait_just(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.wait(Self::token(), timeout, false)
    }

    fn wait(&self, token: usize, timeout: Option<Duration>, schedule: bool) -> std::io::Result<()>;
}

#[derive(Debug)]
pub struct EventLoopImpl<'e> {
    cpu: usize,
    stop: Arc<(Mutex<bool>, Condvar)>,
    pool: CoroutinePoolImpl<'e>,
    selector: SelectorImpl,
    wait_threads: DashMap<usize, Arc<(Mutex<bool>, Condvar)>>,
}

impl EventLoopImpl<'_> {
    fn map_name<'c>(
        wait_table: &DashMap<usize, Arc<(Mutex<bool>, Condvar)>>,
        token: usize,
    ) -> Option<&'c str> {
        if wait_table.contains_key(&token) {
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
            uuid::Uuid::new_v4().to_string(),
            1,
            crate::coroutine::DEFAULT_STACK_SIZE,
            0,
            65536,
            0,
            crate::blocker::DelayBlocker::default(),
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
        let token = Self::token();
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
                self.wait_event(token, Some(left.min(once)))?;
                if let Some(r) = self.try_get_result(task_name) {
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
        assert_eq!(PoolState::Created, self.change_state(PoolState::Running));
        let arc = Arc::new(self);
        let consumer = arc.clone();
        let join_handle = std::thread::Builder::new()
            .name(format!("{}-event-loop", arc.get_name()))
            .spawn(move || {
                // thread per core
                _ = core_affinity::set_for_current(core_affinity::CoreId { id: consumer.cpu });
                let token = Self::token();
                while PoolState::Running == consumer.get_state()
                    || !consumer.is_empty()
                    || consumer.get_running_size() > 0
                {
                    _ = consumer.wait_event(token, Some(Duration::from_millis(10)));
                }
                let (lock, cvar) = &*consumer.stop.clone();
                let mut pending = lock.lock().unwrap();
                *pending = false;
                // Notify the condvar that the value has changed.
                cvar.notify_one();
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
        let token = Self::token();
        loop {
            if left.is_zero() {
                return Err(Error::new(ErrorKind::TimedOut, "stop timeout !"));
            }
            self.wait_event(token, Some(left.min(once)))?;
            if self.is_empty() && self.get_running_size() == 0 {
                _ = self.change_state(PoolState::Stopped);
                return Ok(());
            }
            left = left.saturating_sub(once);
        }
    }
}

impl Selector for EventLoopImpl<'_> {
    fn select(&self, events: &mut Events, timeout: Option<Duration>) -> std::io::Result<usize> {
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

impl<'e> CoroutinePool<'e, JoinHandleImpl<'e>> for EventLoopImpl<'e> {
    fn new(
        name: String,
        cpu: usize,
        stack_size: usize,
        min_size: usize,
        max_size: usize,
        keep_alive_time: u64,
        blocker: impl Blocker + 'e,
    ) -> std::io::Result<Self> {
        let mut event_loop = EventLoopImpl {
            cpu,
            stop: Arc::new((Mutex::new(false), Condvar::new())),
            pool: CoroutinePoolImpl::new(
                name,
                cpu,
                stack_size,
                min_size,
                max_size,
                keep_alive_time,
                blocker,
            )?,
            selector: SelectorImpl::new()?,
            wait_threads: DashMap::new(),
        };
        event_loop.init();
        Ok(event_loop)
    }

    fn init(&mut self) {}

    fn set_stack_size(&self, stack_size: usize) {
        self.pool.set_stack_size(stack_size);
    }

    fn try_resume(&self, co_name: &'e str) -> std::io::Result<()> {
        self.pool.try_resume(co_name)
    }

    fn pop(&self) -> Option<TaskImpl> {
        self.pool.pop()
    }

    fn try_run(&self) -> Option<()> {
        self.pool.try_run()
    }

    fn grow(&self, should_grow: bool) -> std::io::Result<()> {
        self.pool.grow(should_grow)
    }

    fn try_timeout_schedule(&self, timeout_time: u64) -> std::io::Result<u64> {
        Self::init_current(self);
        let result = self.pool.try_timeout_schedule(timeout_time);
        Self::clean_current();
        result
    }

    fn try_get_result(&self, task_name: &str) -> Option<(String, Result<Option<usize>, &str>)> {
        self.pool.try_get_result(task_name)
    }
}

thread_local! {
    static EVENT_LOOP: RefCell<VecDeque<*const c_void>> = RefCell::new(VecDeque::new());
}

impl<'e> Current<'e> for EventLoopImpl<'e> {
    #[allow(clippy::ptr_as_ptr)]
    fn init_current(current: &Self)
    where
        Self: Sized,
    {
        EVENT_LOOP.with(|s| {
            s.borrow_mut()
                .push_front(current as *const _ as *const c_void);
        });
    }

    fn current() -> Option<&'e Self>
    where
        Self: Sized,
    {
        EVENT_LOOP.with(|s| {
            s.borrow()
                .front()
                .map(|ptr| unsafe { &*(*ptr).cast::<EventLoopImpl<'e>>() })
        })
    }

    fn clean_current()
    where
        Self: Sized,
    {
        EVENT_LOOP.with(|s| _ = s.borrow_mut().pop_front());
    }
}

impl<'e> EventLoop<'e> for EventLoopImpl<'e> {
    #[allow(clippy::too_many_lines)]
    fn wait(&self, token: usize, timeout: Option<Duration>, schedule: bool) -> std::io::Result<()> {
        if SchedulableCoroutine::current().is_none() && !self.wait_threads.contains_key(&token) {
            _ = self
                .wait_threads
                .insert(token, Arc::new((Mutex::new(true), Condvar::new())));
        }
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
        let timeout = if schedule {
            Some(
                timeout
                    .map(|time| {
                        if let Some(coroutine) = SchedulableCoroutine::current() {
                            if let Some(suspender) = SchedulableSuspender::current() {
                                let timeout_time = open_coroutine_timer::get_timeout_time(time);
                                let syscall = match coroutine.state() {
                                    CoroutineState::Running => net_syscall,
                                    CoroutineState::SystemCall((), syscall, _) => syscall,
                                    _ => unreachable!("wait should never execute to here"),
                                };
                                coroutine
                                    .syscall((), syscall, SyscallState::Suspend(timeout_time))
                                    .expect("change to syscall state failed !");
                                //协程环境只yield，不执行任务
                                suspender.delay(time);
                                return Ok(Duration::ZERO);
                            }
                        }
                        self.try_timed_schedule(time).map(Duration::from_nanos)
                    })
                    .unwrap()?,
            )
        } else {
            timeout
        };
        if let Some(coroutine) = SchedulableCoroutine::current() {
            match coroutine.state() {
                CoroutineState::SystemCall((), syscall, SyscallState::Timeout) => {
                    coroutine
                        .syscall((), syscall, SyscallState::Calling(net_syscall))
                        .expect("change to syscall state failed !");
                }
                _ => unreachable!("wait should never execute to here"),
            };
        }
        let mut events = Events::new();
        let count = self.selector.select(&mut events, timeout).map_err(|e| {
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
            e
        })?;
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
        if count > 0 {
            //遍历events
            for event in events.iter() {
                if let Some(co_name) = Self::map_name(&self.wait_threads, event.key) {
                    //notify coroutine
                    self.try_resume(co_name).expect("has bug, notice !");
                } else if let Some(arc) = self.wait_threads.get(&event.key) {
                    //notify thread
                    let (lock, cvar) = &*arc.clone();
                    let mut pending = lock.lock().unwrap();
                    *pending = false;
                    cvar.notify_one();
                }
            }
        } else if let Some(arc) = self.wait_threads.get(&token) {
            let (lock, cvar) = &*arc.clone();
            timeout.map_or_else(
                || {
                    drop(cvar.wait_while(lock.lock().unwrap(), |&mut pending| pending));
                    _ = self.wait_threads.remove(&token);
                },
                |wait_time| {
                    let r = cvar
                        .wait_timeout_while(lock.lock().unwrap(), wait_time, |&mut pending| pending)
                        .unwrap();
                    if !r.1.timed_out() {
                        _ = self.wait_threads.remove(&token);
                    }
                    drop(r);
                },
            );
            let mut pending = lock.lock().unwrap();
            *pending = true;
            cvar.notify_one();
        }
        Ok(())
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
