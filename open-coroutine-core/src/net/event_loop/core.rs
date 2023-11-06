use crate::common::{Current, JoinHandle, Named};
use crate::constants::{CoroutineState, PoolState, Syscall, SyscallState};
use crate::coroutine::suspender::SimpleDelaySuspender;
use crate::coroutine::StateCoroutine;
use crate::net::event_loop::join::JoinHandleImpl;
use crate::net::selector::has::HasSelector;
use crate::net::selector::{Event, Events, Selector, SelectorImpl};
use crate::pool::has::HasCoroutinePool;
use crate::pool::task::Task;
use crate::pool::{
    AutoConsumableTaskPool, CoroutinePool, CoroutinePoolImpl, SubmittableTaskPool, TaskPool,
    WaitableTaskPool,
};
use crate::scheduler::has::HasScheduler;
use crate::scheduler::{SchedulableCoroutine, SchedulableSuspender};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{Error, ErrorKind};
use std::panic::RefUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub trait EventLoop<'e, Join: JoinHandle<Self>>:
    AutoConsumableTaskPool<'e, Join> + WaitableTaskPool<'e, Join> + HasCoroutinePool<'e> + HasSelector
{
    fn wait_event(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        let left_time = if SchedulableCoroutine::current().is_some() {
            timeout
        } else if let Some(time) = timeout {
            Some(
                self.pool()
                    .try_timed_schedule_task(time)
                    .map(Duration::from_nanos)?,
            )
        } else {
            self.pool().try_schedule_task()?;
            None
        };
        self.wait_just(left_time)
    }

    fn wait_just(&self, timeout: Option<Duration>) -> std::io::Result<()>;
}

#[allow(trivial_numeric_casts, clippy::cast_possible_truncation)]
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
                    let thread_id = windows_sys::Win32::System::Threading::GetCurrentThread();
                } else {
                    let thread_id = libc::pthread_self();
                }
            }
            thread_id as usize
        }
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
    #[allow(clippy::cast_possible_truncation)]
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
                crate::common::DelayBlocker::default(),
            ),
            selector: SelectorImpl::new()?,
            stop: Arc::new((Mutex::new(false), Condvar::new())),
            shared_stop,
        })
    }

    fn map_name<'c>(token: usize) -> Option<&'c str> {
        unsafe { CStr::from_ptr((token as *const c_void).cast::<c_char>()) }
            .to_str()
            .ok()
    }

    pub fn add_read(&self, fd: c_int) -> std::io::Result<()> {
        self.add_read_event(fd, token())
    }

    pub fn add_write(&self, fd: c_int) -> std::io::Result<()> {
        self.add_write_event(fd, token())
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
            crate::constants::DEFAULT_STACK_SIZE,
            0,
            65536,
            0,
            Arc::new((Mutex::new(AtomicUsize::new(0)), Condvar::new())),
        )
        .expect("create event-loop failed")
    }
}

impl<'e> SubmittableTaskPool<'e, JoinHandleImpl<'e>> for EventLoopImpl<'e> {
    fn submit_raw(&self, task: Task<'e>) {
        self.pool.submit_raw(task);
    }
}

impl<'e> WaitableTaskPool<'e, JoinHandleImpl<'e>> for EventLoopImpl<'e> {
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
                self.wait_event(Some(left.min(once)))?;
                if let Some(r) = self.pool.try_get_task_result(task_name) {
                    return Ok(Some(r));
                }
            }
            left = left.saturating_sub(once);
        }
    }
}

impl<'e> AutoConsumableTaskPool<'e, JoinHandleImpl<'e>> for EventLoopImpl<'e> {
    fn start(self) -> std::io::Result<Arc<Self>>
    where
        'e: 'static,
    {
        assert_eq!(PoolState::Created, self.change_state(PoolState::Running));
        let arc = Arc::new(self);
        let consumer = arc.clone();
        let join_handle = std::thread::Builder::new()
            .name(arc.get_name().to_string())
            .spawn(move || {
                // thread per core
                _ = core_affinity::set_for_current(core_affinity::CoreId { id: consumer.cpu });
                while PoolState::Running == consumer.get_state()
                    || consumer.has_task()
                    || consumer.get_running_size() > 0
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
            self.wait_event(Some(left.min(once)))?;
            if !self.has_task() && self.get_running_size() == 0 {
                _ = self.change_state(PoolState::Stopped);
                return Ok(());
            }
            left = left.saturating_sub(once);
        }
    }
}

impl<'e> HasCoroutinePool<'e> for EventLoopImpl<'e> {
    fn pool(&self) -> &CoroutinePoolImpl<'e> {
        &self.pool
    }

    fn pool_mut(&mut self) -> &mut CoroutinePoolImpl<'e> {
        &mut self.pool
    }
}

impl HasSelector for EventLoopImpl<'_> {
    fn selector(&self) -> &SelectorImpl {
        &self.selector
    }
}

impl<'e> EventLoop<'e, JoinHandleImpl<'e>> for EventLoopImpl<'e> {
    fn wait_just(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        let mut timeout = timeout;
        if let Some(time) = timeout {
            if let Some(coroutine) = SchedulableCoroutine::current() {
                if let Some(suspender) = SchedulableSuspender::current() {
                    let syscall = match coroutine.state() {
                        CoroutineState::Running => {
                            cfg_if::cfg_if! {
                                if #[cfg(all(target_os = "linux", feature = "io_uring"))] {
                                    Syscall::io_uring_enter
                                } else if #[cfg(target_os = "linux")] {
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
                        .syscall(
                            (),
                            syscall,
                            SyscallState::Suspend(open_coroutine_timer::get_timeout_time(time)),
                        )
                        .expect("change to syscall state failed !");
                    suspender.delay(time);
                    //协程环境delay后直接重置timeout
                    timeout = Some(Duration::ZERO);
                }
            }
        }
        let mut events = Events::with_capacity(1024);
        self.select(&mut events, timeout)?;
        //遍历events
        for event in &events {
            if let Some(co_name) = Self::map_name(event.get_token()) {
                //notify coroutine
                self.try_resume(co_name).expect("has bug, notice !");
            }
        }
        Ok(())
    }
}
