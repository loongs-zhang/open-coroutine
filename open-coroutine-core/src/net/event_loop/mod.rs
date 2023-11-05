use crate::common::{Current, JoinHandle, Named};
use crate::constants::{CoroutineState, PoolState, Syscall, SyscallState};
use crate::coroutine::suspender::SimpleDelaySuspender;
use crate::coroutine::StateCoroutine;
use crate::net::event_loop::join::JoinHandleImpl;
use crate::pool::has::HasCoroutinePool;
use crate::pool::task::Task;
use crate::pool::{
    AutoConsumableTaskPool, CoroutinePool, CoroutinePoolImpl, SubmittableTaskPool, TaskPool,
    WaitableTaskPool,
};
use crate::scheduler::has::HasScheduler;
use crate::scheduler::{SchedulableCoroutine, SchedulableSuspender};
use once_cell::sync::Lazy;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{Error, ErrorKind};
use std::panic::RefUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "io_uring"))] {
        use crate::coroutine::suspender::SimpleSuspender;
        use crate::net::operator::Operator;
        use dashmap::DashMap;
        use libc::{
            c_uint, epoll_event, iovec, mode_t, msghdr, off_t, size_t, sockaddr, socklen_t, ssize_t,
        };
    }
}

pub mod join;

#[cfg(test)]
mod tests;

pub trait EventLoop<'e, Join: JoinHandle<Self>>:
    AutoConsumableTaskPool<'e, Join> + WaitableTaskPool<'e, Join> + HasCoroutinePool<'e>
{
    fn wait_event(&self, timeout: Option<Duration>) -> std::io::Result<usize> {
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

    fn wait_just(&self, timeout: Option<Duration>) -> std::io::Result<usize>;
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
    #[cfg(all(target_os = "linux", feature = "io_uring"))]
    operator: Operator<'e>,
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
            #[cfg(all(target_os = "linux", feature = "io_uring"))]
            operator: Operator::new(cpu as u32)?,
            stop: Arc::new((Mutex::new(false), Condvar::new())),
            shared_stop,
        })
    }

    fn map_name<'c>(token: usize) -> Option<&'c str> {
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
                _ = self.wait_event(Some(left.min(once)))?;
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
            _ = self.wait_event(Some(left.min(once)))?;
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

#[allow(clippy::type_complexity)]
static SYSCALL_WAIT_TABLE: Lazy<DashMap<usize, Arc<(Mutex<Option<ssize_t>>, Condvar)>>> =
    Lazy::new(DashMap::new);

impl<'e> EventLoop<'e, JoinHandleImpl<'e>> for EventLoopImpl<'e> {
    fn wait_just(&self, timeout: Option<Duration>) -> std::io::Result<usize> {
        let mut count = 0;
        let mut timeout = timeout;
        if let Some(time) = timeout {
            if let Some(coroutine) = SchedulableCoroutine::current() {
                if let Some(suspender) = SchedulableSuspender::current() {
                    let syscall = match coroutine.state() {
                        CoroutineState::Running => {
                            cfg_if::cfg_if! {
                                if #[cfg(all(target_os = "linux", feature = "io_uring"))] {
                                    Syscall::io_uring_enter
                                } else {
                                    Syscall::epoll_wait
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
        cfg_if::cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "io_uring"))] {
                if crate::net::operator::support_io_uring() {
                    // use io_uring
                    let result = self.operator.select(timeout);
                    let mut r = result?;
                    if r.0 > 0 {
                        for cqe in &mut r.1 {
                            let syscall_result = cqe.result();
                            #[allow(clippy::cast_possible_truncation)]
                            let token = cqe.user_data() as usize;
                            // resolve completed read/write tasks
                            if let Some((_, pair)) = SYSCALL_WAIT_TABLE.remove(&token) {
                                let (lock, cvar) = &*pair;
                                let mut pending = lock.lock().unwrap();
                                *pending = Some(syscall_result as ssize_t);
                                // notify the condvar that the value has changed.
                                cvar.notify_one();
                            }
                            if let Some(co_name) = Self::map_name(token) {
                                //notify coroutine
                                self.try_resume(co_name).expect("has bug, notice !");
                            }
                        }
                        count += r.0;
                    }
                }
            }
        }
        Ok(count)
    }
}

#[cfg(all(target_os = "linux", feature = "io_uring"))]
macro_rules! wrap_result {
    ( $self: expr, $syscall:ident, $($arg: expr),* $(,)* ) => {{
        if let Some(coroutine) = SchedulableCoroutine::current() {
            let syscall = match coroutine.state() {
                CoroutineState::Running => Syscall::$syscall,
                CoroutineState::SystemCall((), syscall, _) => syscall,
                _ => unreachable!("should never execute to here"),
            };
            // prevent signal interruption
            coroutine
                .syscall((), syscall, SyscallState::Executing)
                .expect("change to syscall state failed !");
        }
        let user_data = token();
        $self.operator
            .$syscall(user_data, $($arg, )*)
            .map(|()| {
                let arc = Arc::new((Mutex::new(None), Condvar::new()));
                assert!(
                    SYSCALL_WAIT_TABLE.insert(user_data, arc.clone()).is_none(),
                    "The previous token was not retrieved in a timely manner"
                );
                if let Some(suspender) = SchedulableSuspender::current() {
                    suspender.suspend();
                    // the syscall is done after callback
                }
                let (lock, cvar) = &*arc;
                let syscall_result = cvar
                    .wait_while(lock.lock().unwrap(), |&mut pending| pending.is_none())
                        .unwrap()
                        .unwrap();
                syscall_result as _
            })
    }};
}

#[allow(trivial_numeric_casts)]
#[cfg(all(target_os = "linux", feature = "io_uring"))]
impl EventLoopImpl<'_> {
    pub fn epoll_ctl(
        &self,
        epfd: c_int,
        op: c_int,
        fd: c_int,
        event: *mut epoll_event,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, epoll_ctl, epfd, op, fd, event)
    }

    pub fn openat(
        &self,
        dir_fd: c_int,
        pathname: *const c_char,
        flags: c_int,
        mode: mode_t,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, openat, dir_fd, pathname, flags, mode)
    }

    pub fn mkdirat(
        &self,
        dir_fd: c_int,
        pathname: *const c_char,
        mode: mode_t,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, mkdirat, dir_fd, pathname, mode)
    }

    pub fn renameat(
        &self,
        old_dir_fd: c_int,
        old_path: *const c_char,
        new_dir_fd: c_int,
        new_path: *const c_char,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, renameat, old_dir_fd, old_path, new_dir_fd, new_path)
    }

    pub fn renameat2(
        &self,
        old_dir_fd: c_int,
        old_path: *const c_char,
        new_dir_fd: c_int,
        new_path: *const c_char,
        flags: c_uint,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, renameat2, old_dir_fd, old_path, new_dir_fd, new_path, flags)
    }

    pub fn fsync(&self, fd: c_int) -> std::io::Result<c_int> {
        wrap_result!(self, fsync, fd)
    }

    pub fn socket(&self, domain: c_int, ty: c_int, protocol: c_int) -> std::io::Result<c_int> {
        wrap_result!(self, socket, domain, ty, protocol)
    }

    pub fn accept(
        &self,
        socket: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, accept, socket, address, address_len)
    }

    pub fn accept4(
        &self,
        fd: c_int,
        addr: *mut sockaddr,
        len: *mut socklen_t,
        flg: c_int,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, accept4, fd, addr, len, flg)
    }

    pub fn connect(
        &self,
        socket: c_int,
        address: *const sockaddr,
        len: socklen_t,
    ) -> std::io::Result<c_int> {
        wrap_result!(self, connect, socket, address, len)
    }

    pub fn shutdown(&self, socket: c_int, how: c_int) -> std::io::Result<c_int> {
        wrap_result!(self, shutdown, socket, how)
    }

    pub fn close(&self, fd: c_int) -> std::io::Result<c_int> {
        wrap_result!(self, close, fd)
    }

    pub fn recv(
        &self,
        socket: c_int,
        buf: *mut c_void,
        len: size_t,
        flags: c_int,
    ) -> std::io::Result<ssize_t> {
        wrap_result!(self, recv, socket, buf, len, flags)
    }

    pub fn read(&self, fd: c_int, buf: *mut c_void, count: size_t) -> std::io::Result<ssize_t> {
        wrap_result!(self, read, fd, buf, count)
    }

    pub fn pread(
        &self,
        fd: c_int,
        buf: *mut c_void,
        count: size_t,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        wrap_result!(self, pread, fd, buf, count, offset)
    }

    pub fn readv(&self, fd: c_int, iov: *const iovec, iovcnt: c_int) -> std::io::Result<ssize_t> {
        wrap_result!(self, readv, fd, iov, iovcnt)
    }

    pub fn preadv(
        &self,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        wrap_result!(self, preadv, fd, iov, iovcnt, offset)
    }

    pub fn recvmsg(&self, fd: c_int, msg: *mut msghdr, flags: c_int) -> std::io::Result<ssize_t> {
        wrap_result!(self, recvmsg, fd, msg, flags)
    }

    pub fn send(
        &self,
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
    ) -> std::io::Result<ssize_t> {
        wrap_result!(self, send, socket, buf, len, flags)
    }

    pub fn sendto(
        &self,
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
        addr: *const sockaddr,
        addrlen: socklen_t,
    ) -> std::io::Result<ssize_t> {
        wrap_result!(self, sendto, socket, buf, len, flags, addr, addrlen)
    }

    pub fn write(&self, fd: c_int, buf: *const c_void, count: size_t) -> std::io::Result<ssize_t> {
        wrap_result!(self, write, fd, buf, count)
    }

    pub fn pwrite(
        &self,
        fd: c_int,
        buf: *const c_void,
        count: size_t,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        wrap_result!(self, pwrite, fd, buf, count, offset)
    }

    pub fn writev(&self, fd: c_int, iov: *const iovec, iovcnt: c_int) -> std::io::Result<ssize_t> {
        wrap_result!(self, writev, fd, iov, iovcnt)
    }

    pub fn pwritev(
        &self,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        wrap_result!(self, pwritev, fd, iov, iovcnt, offset)
    }

    pub fn sendmsg(&self, fd: c_int, msg: *const msghdr, flags: c_int) -> std::io::Result<ssize_t> {
        wrap_result!(self, sendmsg, fd, msg, flags)
    }
}
