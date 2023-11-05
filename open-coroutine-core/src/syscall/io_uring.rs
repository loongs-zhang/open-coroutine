use crate::syscall::LinuxSyscall;
use crate::syscall::UnixSyscall;
use libc::epoll_event;
use libc::{
    c_int, c_uint, c_void, fd_set, iovec, msghdr, nfds_t, off_t, pollfd, size_t, sockaddr,
    socklen_t, ssize_t, timespec, timeval,
};

#[derive(Debug, Default)]
pub struct IoUringLinuxSyscall<I: UnixSyscall> {
    inner: I,
}

macro_rules! impl_default {
    ( $self: expr, $syscall:ident, $fn_ptr:expr, $($arg: expr),* $(,)* ) => {{
        $self.inner.$syscall($fn_ptr, $($arg, )*)
    }};
}

macro_rules! impl_io_uring {
    ( $self: expr, $syscall:ident, $fn_ptr:expr, $($arg: expr),* $(,)* ) => {{
        if let Ok(result) = crate::net::core::EventLoops::$syscall($($arg, )*) {
            return result;
        }
        impl_default!($self, $syscall, $fn_ptr, $($arg, )*)
    }};
}

impl<I: UnixSyscall> UnixSyscall for IoUringLinuxSyscall<I> {
    fn sleep(&self, fn_ptr: Option<&extern "C" fn(c_uint) -> c_uint>, secs: c_uint) -> c_uint {
        impl_default!(self, sleep, fn_ptr, secs)
    }

    fn usleep(
        &self,
        fn_ptr: Option<&extern "C" fn(c_uint) -> c_int>,
        microseconds: c_uint,
    ) -> c_int {
        impl_default!(self, usleep, fn_ptr, microseconds)
    }

    fn nanosleep(
        &self,
        fn_ptr: Option<&extern "C" fn(*const timespec, *mut timespec) -> c_int>,
        rqtp: *const timespec,
        rmtp: *mut timespec,
    ) -> c_int {
        impl_default!(self, nanosleep, fn_ptr, rqtp, rmtp)
    }

    fn poll(
        &self,
        fn_ptr: Option<&extern "C" fn(*mut pollfd, nfds_t, c_int) -> c_int>,
        fds: *mut pollfd,
        nfds: nfds_t,
        timeout: c_int,
    ) -> c_int {
        impl_default!(self, poll, fn_ptr, fds, nfds, timeout)
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
        impl_default!(self, select, fn_ptr, nfds, readfds, writefds, errorfds, timeout)
    }

    fn socket(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int, c_int) -> c_int>,
        domain: c_int,
        ty: c_int,
        protocol: c_int,
    ) -> c_int {
        impl_io_uring!(self, socket, fn_ptr, domain, ty, protocol)
    }

    fn listen(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int) -> c_int>,
        socket: c_int,
        backlog: c_int,
    ) -> c_int {
        impl_default!(self, listen, fn_ptr, socket, backlog)
    }

    fn accept(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut sockaddr, *mut socklen_t) -> c_int>,
        socket: c_int,
        address: *mut sockaddr,
        address_len: *mut socklen_t,
    ) -> c_int {
        impl_io_uring!(self, accept, fn_ptr, socket, address, address_len)
    }

    fn connect(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const sockaddr, socklen_t) -> c_int>,
        socket: c_int,
        address: *const sockaddr,
        len: socklen_t,
    ) -> c_int {
        //todo
        impl_default!(self, connect, fn_ptr, socket, address, len)
    }

    fn shutdown(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int) -> c_int>,
        socket: c_int,
        how: c_int,
    ) -> c_int {
        impl_io_uring!(self, shutdown, fn_ptr, socket, how)
    }

    fn close(&self, fn_ptr: Option<&extern "C" fn(c_int) -> c_int>, fd: c_int) -> c_int {
        impl_io_uring!(self, close, fn_ptr, fd)
    }

    fn recv(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t, c_int) -> ssize_t>,
        socket: c_int,
        buf: *mut c_void,
        len: size_t,
        flags: c_int,
    ) -> ssize_t {
        //todo
        impl_default!(self, recv, fn_ptr, socket, buf, len, flags)
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
        impl_default!(self, recvfrom, fn_ptr, socket, buf, len, flags, addr, addrlen)
    }

    fn read(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t) -> ssize_t>,
        fd: c_int,
        buf: *mut c_void,
        count: size_t,
    ) -> ssize_t {
        impl_io_uring!(self, read, fn_ptr, fd, buf, count)
    }

    fn pread(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t, off_t) -> ssize_t>,
        fd: c_int,
        buf: *mut c_void,
        count: size_t,
        offset: off_t,
    ) -> ssize_t {
        impl_io_uring!(self, pread, fn_ptr, fd, buf, count, offset)
    }

    fn readv(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
    ) -> ssize_t {
        impl_io_uring!(self, readv, fn_ptr, fd, iov, iovcnt)
    }

    fn preadv(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int, off_t) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> ssize_t {
        impl_io_uring!(self, preadv, fn_ptr, fd, iov, iovcnt, offset)
    }

    fn recvmsg(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut msghdr, c_int) -> ssize_t>,
        fd: c_int,
        msg: *mut msghdr,
        flags: c_int,
    ) -> ssize_t {
        impl_io_uring!(self, recvmsg, fn_ptr, fd, msg, flags)
    }

    fn send(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t, c_int) -> ssize_t>,
        socket: c_int,
        buf: *const c_void,
        len: size_t,
        flags: c_int,
    ) -> ssize_t {
        //todo
        impl_default!(self, send, fn_ptr, socket, buf, len, flags)
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
        impl_io_uring!(self, sendto, fn_ptr, socket, buf, len, flags, addr, addrlen)
    }

    fn write(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t) -> ssize_t>,
        fd: c_int,
        buf: *const c_void,
        count: size_t,
    ) -> ssize_t {
        impl_io_uring!(self, write, fn_ptr, fd, buf, count)
    }

    fn pwrite(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t, off_t) -> ssize_t>,
        fd: c_int,
        buf: *const c_void,
        count: size_t,
        offset: off_t,
    ) -> ssize_t {
        impl_io_uring!(self, pwrite, fn_ptr, fd, buf, count, offset)
    }

    fn writev(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
    ) -> ssize_t {
        impl_io_uring!(self, writev, fn_ptr, fd, iov, iovcnt)
    }

    fn pwritev(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int, off_t) -> ssize_t>,
        fd: c_int,
        iov: *const iovec,
        iovcnt: c_int,
        offset: off_t,
    ) -> ssize_t {
        impl_io_uring!(self, pwritev, fn_ptr, fd, iov, iovcnt, offset)
    }

    fn sendmsg(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *const msghdr, c_int) -> ssize_t>,
        fd: c_int,
        msg: *const msghdr,
        flags: c_int,
    ) -> ssize_t {
        impl_io_uring!(self, sendmsg, fn_ptr, fd, msg, flags)
    }
}

impl<I: LinuxSyscall> LinuxSyscall for IoUringLinuxSyscall<I> {
    fn epoll_ctl(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, c_int, c_int, *mut epoll_event) -> c_int>,
        epfd: c_int,
        op: c_int,
        fd: c_int,
        event: *mut epoll_event,
    ) -> c_int {
        impl_io_uring!(self, epoll_ctl, fn_ptr, epfd, op, fd, event)
    }

    fn accept4(
        &self,
        fn_ptr: Option<&extern "C" fn(c_int, *mut sockaddr, *mut socklen_t, c_int) -> c_int>,
        fd: c_int,
        addr: *mut sockaddr,
        len: *mut socklen_t,
        flg: c_int,
    ) -> c_int {
        impl_io_uring!(self, accept4, fn_ptr, fd, addr, len, flg)
    }
}
