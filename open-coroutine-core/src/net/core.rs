use crate::constants::MONITOR_CPU;
use crate::coroutine::suspender::Suspender;
#[cfg(all(unix, feature = "preemptive-schedule"))]
use crate::monitor::Monitor;
use crate::net::config::Config;
use crate::net::event_loop::{EventLoop, EventLoopImpl};
#[cfg(all(unix, feature = "preemptive-schedule"))]
use crate::pool::task::Task;
use crate::pool::{AutoConsumableTaskPool, SubmittableTaskPool};
use crate::scheduler::has::HasScheduler;
#[cfg(all(target_os = "linux", feature = "io_uring"))]
use libc::{
    c_char, c_uint, c_void, epoll_event, iovec, mode_t, msghdr, off_t, size_t, sockaddr, socklen_t,
    ssize_t,
};
use once_cell::sync::Lazy;
use std::ffi::c_int;
use std::panic::UnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
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
        if skip_monitor && index % EVENT_LOOPS.len() == MONITOR_CPU {
            INDEX.store(1, Ordering::Release);
            (
                EVENT_LOOPS.get(1).expect("init event-loop-1 failed!"),
                false,
            )
        } else {
            index %= EVENT_LOOPS.len();
            cfg_if::cfg_if! {
                if #[cfg(all(unix, feature = "preemptive-schedule"))] {
                    let is_monitor = MONITOR_CPU == index;
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
            .get(MONITOR_CPU)
            .expect("init event-loop-monitor failed!")
    }

    pub fn stop() {
        crate::warn!("open-coroutine is exiting...");
        #[cfg(all(unix, feature = "preemptive-schedule"))]
        crate::monitor::MonitorImpl::get_instance().stop();
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
    ) -> crate::net::event_loop::join::JoinHandleImpl<'static> {
        EventLoops::next(true).0.submit(name, f, param)
    }

    pub fn submit_co(
        f: impl FnOnce(&dyn Suspender<Resume = (), Yield = ()>, ()) -> Option<usize>
            + UnwindSafe
            + 'static,
        param: Option<usize>,
    ) -> std::io::Result<crate::scheduler::join::JoinHandleImpl<'static>> {
        EventLoops::next(true).0.submit_co(f, param)
    }

    #[cfg(all(unix, feature = "preemptive-schedule"))]
    pub(crate) fn submit_raw(task: Task<'static>) {
        EventLoops::next(true).0.submit_raw(task);
    }

    pub fn wait_event(timeout: Option<Duration>) -> std::io::Result<usize> {
        EventLoops::next(true).0.wait_event(timeout)
    }
}

#[cfg(all(target_os = "linux", feature = "io_uring"))]
impl EventLoops {
    pub fn epoll_ctl(
        epfd: c_int,
        op: c_int,
        fd: c_int,
        event: *mut epoll_event,
    ) -> std::io::Result<c_int> {
        EventLoops::next(true).0.epoll_ctl(epfd, op, fd, event)
    }

    pub fn openat(
        dir_fd: c_int,
        pathname: *const c_char,
        flags: c_int,
        mode: mode_t,
    ) -> std::io::Result<c_int> {
        EventLoops::next(true)
            .0
            .openat(dir_fd, pathname, flags, mode)
    }

    pub fn mkdirat(dir_fd: c_int, pathname: *const c_char, mode: mode_t) -> std::io::Result<c_int> {
        EventLoops::next(true).0.mkdirat(dir_fd, pathname, mode)
    }

    pub fn renameat(
        old_dir_fd: c_int,
        old_path: *const c_char,
        new_dir_fd: c_int,
        new_path: *const c_char,
    ) -> std::io::Result<c_int> {
        EventLoops::next(true)
            .0
            .renameat(old_dir_fd, old_path, new_dir_fd, new_path)
    }

    pub fn renameat2(
        old_dir_fd: c_int,
        old_path: *const c_char,
        new_dir_fd: c_int,
        new_path: *const c_char,
        flags: c_uint,
    ) -> std::io::Result<c_int> {
        EventLoops::next(true)
            .0
            .renameat2(old_dir_fd, old_path, new_dir_fd, new_path, flags)
    }

    pub fn fsync(fd: c_int) -> std::io::Result<c_int> {
        EventLoops::next(true).0.fsync(fd)
    }

    pub fn socket(domain: c_int, ty: c_int, protocol: c_int) -> std::io::Result<c_int> {
        EventLoops::next(true).0.socket(domain, ty, protocol)
    }

    pub fn accept(
        socket: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> std::io::Result<c_int> {
        EventLoops::next(true)
            .0
            .accept(socket, address, address_len)
    }

    pub fn accept4(
        fd: c_int,
        addr: *mut sockaddr,
        len: *mut socklen_t,
        flg: c_int,
    ) -> std::io::Result<c_int> {
        EventLoops::next(true).0.accept4(fd, addr, len, flg)
    }

    pub fn connect(
        socket: c_int,
        address: *const sockaddr,
        len: socklen_t,
    ) -> std::io::Result<c_int> {
        EventLoops::next(true).0.connect(socket, address, len)
    }

    pub fn shutdown(socket: c_int, how: c_int) -> std::io::Result<c_int> {
        EventLoops::next(true).0.shutdown(socket, how)
    }

    pub fn close(fd: c_int) -> std::io::Result<c_int> {
        EventLoops::next(true).0.close(fd)
    }

    pub fn recv(
        socket: c_int,
        buf: *mut c_void,
        len: size_t,
        flags: c_int,
    ) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.recv(socket, buf, len, flags)
    }

    pub fn read(fd: c_int, buf: *mut c_void, count: size_t) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.read(fd, buf, count)
    }

    pub fn pread(
        fd: c_int,
        buf: *mut c_void,
        count: size_t,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.pread(fd, buf, count, offset)
    }

    pub fn readv(fd: c_int, iov: *const iovec, iovcnt: c_int) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.readv(fd, iov, iovcnt)
    }

    pub fn preadv(
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.preadv(fd, iov, iovcnt, offset)
    }

    pub fn recvmsg(fd: c_int, msg: *mut msghdr, flags: c_int) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.recvmsg(fd, msg, flags)
    }

    pub fn send(
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
    ) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.send(socket, buf, len, flags)
    }

    pub fn sendto(
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
        addr: *const sockaddr,
        addrlen: socklen_t,
    ) -> std::io::Result<ssize_t> {
        EventLoops::next(true)
            .0
            .sendto(socket, buf, len, flags, addr, addrlen)
    }

    pub fn write(fd: c_int, buf: *const c_void, count: size_t) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.write(fd, buf, count)
    }

    pub fn pwrite(
        fd: c_int,
        buf: *const c_void,
        count: size_t,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.pwrite(fd, buf, count, offset)
    }

    pub fn writev(fd: c_int, iov: *const iovec, iovcnt: c_int) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.writev(fd, iov, iovcnt)
    }

    pub fn pwritev(
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.pwritev(fd, iov, iovcnt, offset)
    }

    pub fn sendmsg(fd: c_int, msg: *const msghdr, flags: c_int) -> std::io::Result<ssize_t> {
        EventLoops::next(true).0.sendmsg(fd, msg, flags)
    }
}
