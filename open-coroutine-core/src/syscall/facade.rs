use crate::syscall::raw::RawLinuxSyscall;
use crate::syscall::state::StateLinuxSyscall;
#[cfg(target_os = "linux")]
use crate::syscall::LinuxSyscall;
use crate::syscall::UnixSyscall;
#[cfg(target_os = "linux")]
use libc::epoll_event;
use libc::{
    c_int, c_uint, c_void, fd_set, iovec, msghdr, nfds_t, off_t, pollfd, size_t, sockaddr,
    socklen_t, ssize_t, timespec, timeval,
};
use once_cell::sync::Lazy;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "io_uring"))] {
        use crate::syscall::io_uring::IoUringLinuxSyscall;
        static CHAIN: Lazy<StateLinuxSyscall<IoUringLinuxSyscall<RawLinuxSyscall>>> =
            Lazy::new(StateLinuxSyscall::default);
    } else {
        static CHAIN: Lazy<StateLinuxSyscall<NioLinuxSyscall<RawLinuxSyscall>>> =
            Lazy::new(StateLinuxSyscall::default);
    }
}

/// sleep

#[must_use]
pub fn sleep(fn_ptr: Option<&extern "C" fn(c_uint) -> c_uint>, secs: c_uint) -> c_uint {
    CHAIN.sleep(fn_ptr, secs)
}

#[must_use]
pub fn usleep(fn_ptr: Option<&extern "C" fn(c_uint) -> c_int>, microseconds: c_uint) -> c_int {
    CHAIN.usleep(fn_ptr, microseconds)
}

#[must_use]
pub fn nanosleep(
    fn_ptr: Option<&extern "C" fn(*const timespec, *mut timespec) -> c_int>,
    rqtp: *const timespec,
    rmtp: *mut timespec,
) -> c_int {
    CHAIN.nanosleep(fn_ptr, rqtp, rmtp)
}

/// poll

#[must_use]
pub fn poll(
    fn_ptr: Option<&extern "C" fn(*mut pollfd, nfds_t, c_int) -> c_int>,
    fds: *mut pollfd,
    count: nfds_t,
    timeout: c_int,
) -> c_int {
    CHAIN.poll(fn_ptr, fds, count, timeout)
}

#[must_use]
pub fn select(
    fn_ptr: Option<
        &extern "C" fn(c_int, *mut fd_set, *mut fd_set, *mut fd_set, *mut timeval) -> c_int,
    >,
    nfds: c_int,
    readfds: *mut fd_set,
    writefds: *mut fd_set,
    errorfds: *mut fd_set,
    timeout: *mut timeval,
) -> c_int {
    CHAIN.select(fn_ptr, nfds, readfds, writefds, errorfds, timeout)
}

/// socket

#[must_use]
pub fn socket(
    fn_ptr: Option<&extern "C" fn(c_int, c_int, c_int) -> c_int>,
    domain: c_int,
    ty: c_int,
    protocol: c_int,
) -> c_int {
    CHAIN.socket(fn_ptr, domain, ty, protocol)
}

#[must_use]
pub fn listen(
    fn_ptr: Option<&extern "C" fn(c_int, c_int) -> c_int>,
    socket: c_int,
    backlog: c_int,
) -> c_int {
    CHAIN.listen(fn_ptr, socket, backlog)
}

#[must_use]
pub fn accept(
    fn_ptr: Option<&extern "C" fn(c_int, *mut sockaddr, *mut socklen_t) -> c_int>,
    socket: c_int,
    address: *mut sockaddr,
    address_len: *mut socklen_t,
) -> c_int {
    CHAIN.accept(fn_ptr, socket, address, address_len)
}

#[must_use]
pub fn connect(
    fn_ptr: Option<&extern "C" fn(c_int, *const sockaddr, socklen_t) -> c_int>,
    socket: c_int,
    address: *const sockaddr,
    len: socklen_t,
) -> c_int {
    CHAIN.connect(fn_ptr, socket, address, len)
}

#[must_use]
pub fn shutdown(
    fn_ptr: Option<&extern "C" fn(c_int, c_int) -> c_int>,
    socket: c_int,
    how: c_int,
) -> c_int {
    CHAIN.shutdown(fn_ptr, socket, how)
}

#[must_use]
pub fn close(fn_ptr: Option<&extern "C" fn(c_int) -> c_int>, fd: c_int) -> c_int {
    CHAIN.close(fn_ptr, fd)
}

/// read

#[must_use]
pub fn recv(
    fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t, c_int) -> ssize_t>,
    socket: c_int,
    buf: *mut c_void,
    len: size_t,
    flags: c_int,
) -> ssize_t {
    CHAIN.recv(fn_ptr, socket, buf, len, flags)
}

#[must_use]
pub fn recvfrom(
    fn_ptr: Option<
        &extern "C" fn(c_int, *mut c_void, size_t, c_int, *mut sockaddr, *mut socklen_t) -> ssize_t,
    >,
    socket: c_int,
    buf: *mut c_void,
    len: size_t,
    flags: c_int,
    addr: *mut sockaddr,
    addrlen: *mut socklen_t,
) -> ssize_t {
    CHAIN.recvfrom(fn_ptr, socket, buf, len, flags, addr, addrlen)
}

#[must_use]
pub fn read(
    fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t) -> ssize_t>,
    fd: c_int,
    buf: *mut c_void,
    count: size_t,
) -> ssize_t {
    CHAIN.read(fn_ptr, fd, buf, count)
}

#[must_use]
pub fn pread(
    fn_ptr: Option<&extern "C" fn(c_int, *mut c_void, size_t, off_t) -> ssize_t>,
    fd: c_int,
    buf: *mut c_void,
    count: size_t,
    offset: off_t,
) -> ssize_t {
    CHAIN.pread(fn_ptr, fd, buf, count, offset)
}

#[must_use]
pub fn readv(
    fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int) -> ssize_t>,
    fd: c_int,
    iov: *const iovec,
    iovcnt: c_int,
) -> ssize_t {
    CHAIN.readv(fn_ptr, fd, iov, iovcnt)
}

#[must_use]
pub fn preadv(
    fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int, off_t) -> ssize_t>,
    fd: c_int,
    iov: *const iovec,
    iovcnt: c_int,
    offset: off_t,
) -> ssize_t {
    CHAIN.preadv(fn_ptr, fd, iov, iovcnt, offset)
}

#[must_use]
pub fn recvmsg(
    fn_ptr: Option<&extern "C" fn(c_int, *mut msghdr, c_int) -> ssize_t>,
    fd: c_int,
    msg: *mut msghdr,
    flags: c_int,
) -> ssize_t {
    CHAIN.recvmsg(fn_ptr, fd, msg, flags)
}

/// write

#[must_use]
pub fn send(
    fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t, c_int) -> ssize_t>,
    socket: c_int,
    buf: *const c_void,
    len: size_t,
    flags: c_int,
) -> ssize_t {
    CHAIN.send(fn_ptr, socket, buf, len, flags)
}

#[must_use]
pub fn sendto(
    fn_ptr: Option<
        &extern "C" fn(c_int, *const c_void, size_t, c_int, *const sockaddr, socklen_t) -> ssize_t,
    >,
    socket: c_int,
    buf: *const c_void,
    len: size_t,
    flags: c_int,
    addr: *const sockaddr,
    addrlen: socklen_t,
) -> ssize_t {
    CHAIN.sendto(fn_ptr, socket, buf, len, flags, addr, addrlen)
}

#[must_use]
pub fn write(
    fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t) -> ssize_t>,
    fd: c_int,
    buf: *const c_void,
    count: size_t,
) -> ssize_t {
    CHAIN.write(fn_ptr, fd, buf, count)
}

#[must_use]
pub fn pwrite(
    fn_ptr: Option<&extern "C" fn(c_int, *const c_void, size_t, off_t) -> ssize_t>,
    fd: c_int,
    buf: *const c_void,
    count: size_t,
    offset: off_t,
) -> ssize_t {
    CHAIN.pwrite(fn_ptr, fd, buf, count, offset)
}

#[must_use]
pub fn writev(
    fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int) -> ssize_t>,
    fd: c_int,
    iov: *const iovec,
    iovcnt: c_int,
) -> ssize_t {
    CHAIN.writev(fn_ptr, fd, iov, iovcnt)
}

#[must_use]
pub fn pwritev(
    fn_ptr: Option<&extern "C" fn(c_int, *const iovec, c_int, off_t) -> ssize_t>,
    fd: c_int,
    iov: *const iovec,
    iovcnt: c_int,
    offset: off_t,
) -> ssize_t {
    CHAIN.pwritev(fn_ptr, fd, iov, iovcnt, offset)
}

#[must_use]
pub fn sendmsg(
    fn_ptr: Option<&extern "C" fn(c_int, *const msghdr, c_int) -> ssize_t>,
    fd: c_int,
    msg: *const msghdr,
    flags: c_int,
) -> ssize_t {
    CHAIN.sendmsg(fn_ptr, fd, msg, flags)
}

/// poll

#[cfg(target_os = "linux")]
#[must_use]
pub fn epoll_ctl(
    fn_ptr: Option<&extern "C" fn(c_int, c_int, c_int, *mut epoll_event) -> c_int>,
    epfd: c_int,
    op: c_int,
    fd: c_int,
    event: *mut epoll_event,
) -> c_int {
    CHAIN.epoll_ctl(fn_ptr, epfd, op, fd, event)
}

/// socket

#[cfg(target_os = "linux")]
#[must_use]
pub fn accept4(
    fn_ptr: Option<&extern "C" fn(c_int, *mut sockaddr, *mut socklen_t, c_int) -> c_int>,
    fd: c_int,
    addr: *mut sockaddr,
    len: *mut socklen_t,
    flg: c_int,
) -> c_int {
    CHAIN.accept4(fn_ptr, fd, addr, len, flg)
}
