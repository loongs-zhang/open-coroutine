use crate::common::Current;
use crate::constants::{Syscall, SyscallState};
use crate::coroutine::StateCoroutine;
use crate::scheduler::SchedulableCoroutine;
#[cfg(target_os = "linux")]
use crate::syscall::LinuxSyscall;
use crate::syscall::UnixSyscall;
#[cfg(target_os = "linux")]
use libc::epoll_event;
use libc::{
    c_int, c_uint, c_void, fd_set, iovec, msghdr, nfds_t, off_t, pollfd, size_t, sockaddr,
    socklen_t, ssize_t, timespec, timeval,
};

#[derive(Debug, Default)]
pub struct StateLinuxSyscall<I: UnixSyscall> {
    inner: I,
}

macro_rules! change_state {
    ( $self: expr, $syscall:ident, $($arg: expr),* $(,)* ) => {{
        Syscall::init_current(Syscall::$syscall);
        if let Some(coroutine) = SchedulableCoroutine::current() {
            //协程进入系统调用状态
            coroutine
                .syscall((), Syscall::$syscall, SyscallState::Executing)
                .expect("change to syscall state failed !");
        }
        let r = $self.inner.$syscall($($arg, )*);
        if let Some(coroutine) = SchedulableCoroutine::current() {
            //系统调用完成
            coroutine
                .syscall((), Syscall::$syscall, SyscallState::Finished)
                .expect("change to syscall Finished state failed !");
        }
        Syscall::clean_current();
        return r;
    }};
}

impl<I: UnixSyscall> UnixSyscall for StateLinuxSyscall<I> {
    fn sleep(&self, fn_ptr: Option<&extern "C" fn(c_uint) -> c_uint>, secs: c_uint) -> c_uint {
        change_state!(self, sleep, fn_ptr, secs)
    }

    fn usleep(
        &self,
        fn_ptr: Option<&extern "C" fn(c_uint) -> c_int>,
        microseconds: c_uint,
    ) -> c_int {
        change_state!(self, usleep, fn_ptr, microseconds)
    }

    fn nanosleep(
        &self,
        fn_ptr: Option<&extern "C" fn(*const timespec, *mut timespec) -> c_int>,
        rqtp: *const timespec,
        rmtp: *mut timespec,
    ) -> c_int {
        change_state!(self, nanosleep, fn_ptr, rqtp, rmtp)
    }

    fn poll(
        &self,
        fn_ptr: Option<&extern "C" fn(*mut pollfd, nfds_t, c_int) -> c_int>,
        fds: *mut pollfd,
        nfds: nfds_t,
        timeout: c_int,
    ) -> c_int {
        change_state!(self, poll, fn_ptr, fds, nfds, timeout)
    }

    fn select(
        &self,
        fn_ptr: Option<
            &extern "C" fn(c_int, *mut fd_set, *mut fd_set, *mut fd_set, *mut timeval) -> c_int,
        >,
        nfds: c_int,
        readfds: *mut fd_set,
        writefds: *mut fd_set,
        errorfds: *mut fd_set,
        timeout: *mut timeval,
    ) -> c_int {
        change_state!(self, select, fn_ptr, nfds, readfds, writefds, errorfds, timeout)
    }

    fn socket(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int, c_int) -> c_int>,
        domain: c_int,
        ty: c_int,
        protocol: c_int,
    ) -> c_int {
        change_state!(self, socket, fn_ptr, domain, ty, protocol)
    }

    fn listen(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int) -> c_int>,
        socket: c_int,
        backlog: c_int,
    ) -> c_int {
        change_state!(self, listen, fn_ptr, socket, backlog)
    }

    fn accept(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut sockaddr, *mut socklen_t) -> c_int>,
        socket: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> c_int {
        change_state!(self, accept, fn_ptr, socket, address, address_len)
    }

    fn connect(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const sockaddr, socklen_t) -> c_int>,
        socket: c_int,
        address: *const sockaddr,
        len: socklen_t,
    ) -> c_int {
        change_state!(self, connect, fn_ptr, socket, address, len)
    }

    fn shutdown(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int) -> c_int>,
        socket: c_int,
        how: c_int,
    ) -> c_int {
        change_state!(self, shutdown, fn_ptr, socket, how)
    }

    fn close(&self, fn_ptr: Option<&extern "C" fn(c_int) -> c_int>, fd: c_int) -> c_int {
        change_state!(self, close, fn_ptr, fd)
    }

    fn recv(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t, c_int) -> ssize_t>,
        socket: c_int,
        buf: *mut c_void,
        len: size_t,
        flags: c_int,
    ) -> ssize_t {
        change_state!(self, recv, fn_ptr, socket, buf, len, flags)
    }

    fn recvfrom(
        &self,
        fn_ptr: Option<
            &extern "C" fn(
                c_int,
                *mut c_void,
                size_t,
                c_int,
                *mut sockaddr,
                *mut socklen_t,
            ) -> ssize_t,
        >,
        socket: c_int,
        buf: *mut c_void,
        len: size_t,
        flags: c_int,
        addr: *mut sockaddr,
        addrlen: *mut socklen_t,
    ) -> ssize_t {
        change_state!(self, recvfrom, fn_ptr, socket, buf, len, flags, addr, addrlen)
    }

    fn read(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t) -> ssize_t>,
        fd: c_int,
        buf: *mut c_void,
        count: size_t,
    ) -> ssize_t {
        change_state!(self, read, fn_ptr, fd, buf, count)
    }

    fn pread(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t, off_t) -> ssize_t>,
        fd: c_int,
        buf: *mut c_void,
        count: size_t,
        offset: off_t,
    ) -> ssize_t {
        change_state!(self, pread, fn_ptr, fd, buf, count, offset)
    }

    fn readv(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
    ) -> ssize_t {
        change_state!(self, readv, fn_ptr, fd, iov, iovcnt)
    }

    fn preadv(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int, off_t) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> ssize_t {
        change_state!(self, preadv, fn_ptr, fd, iov, iovcnt, offset)
    }

    fn recvmsg(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut msghdr, c_int) -> ssize_t>,
        fd: c_int,
        msg: *mut msghdr,
        flags: c_int,
    ) -> ssize_t {
        change_state!(self, recvmsg, fn_ptr, fd, msg, flags)
    }

    fn send(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t, c_int) -> ssize_t>,
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
    ) -> ssize_t {
        change_state!(self, send, fn_ptr, socket, buf, len, flags)
    }

    fn sendto(
        &self,
        fn_ptr: Option<
            &extern "C" fn(
                c_int,
                *const c_void,
                size_t,
                c_int,
                *const sockaddr,
                socklen_t,
            ) -> ssize_t,
        >,
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
        addr: *const sockaddr,
        addrlen: socklen_t,
    ) -> ssize_t {
        change_state!(self, sendto, fn_ptr, socket, buf, len, flags, addr, addrlen)
    }

    fn write(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t) -> ssize_t>,
        fd: c_int,
        buf: *const c_void,
        count: size_t,
    ) -> ssize_t {
        change_state!(self, write, fn_ptr, fd, buf, count)
    }

    fn pwrite(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t, off_t) -> ssize_t>,
        fd: c_int,
        buf: *const c_void,
        count: size_t,
        offset: off_t,
    ) -> ssize_t {
        change_state!(self, pwrite, fn_ptr, fd, buf, count, offset)
    }

    fn writev(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
    ) -> ssize_t {
        change_state!(self, writev, fn_ptr, fd, iov, iovcnt)
    }

    fn pwritev(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int, off_t) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> ssize_t {
        change_state!(self, pwritev, fn_ptr, fd, iov, iovcnt, offset)
    }

    fn sendmsg(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const msghdr, c_int) -> ssize_t>,
        fd: c_int,
        msg: *const msghdr,
        flags: c_int,
    ) -> ssize_t {
        change_state!(self, sendmsg, fn_ptr, fd, msg, flags)
    }
}

#[cfg(target_os = "linux")]
impl<I: LinuxSyscall> LinuxSyscall for StateLinuxSyscall<I> {
    fn epoll_ctl(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int, c_int, *mut epoll_event) -> c_int>,
        epfd: c_int,
        op: c_int,
        fd: c_int,
        event: *mut epoll_event,
    ) -> c_int {
        change_state!(self, epoll_ctl, fn_ptr, epfd, op, fd, event)
    }

    fn accept4(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut sockaddr, *mut socklen_t, c_int) -> c_int>,
        fd: c_int,
        addr: *mut sockaddr,
        len: *mut socklen_t,
        flg: c_int,
    ) -> c_int {
        change_state!(self, accept4, fn_ptr, fd, addr, len, flg)
    }
}
