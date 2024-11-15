use crate::common::constants::Syscall;
use crate::common::{get_timeout_time, now};
use crate::impl_display_by_debug;
use dashmap::{DashMap, DashSet};
use once_cell::sync::Lazy;
use std::ffi::{c_int, c_uint, c_void};
use std::io::{Error, ErrorKind};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows_sys::core::{PCSTR, PSTR};
use windows_sys::Win32::Foundation::{
    ERROR_NETNAME_DELETED, FALSE, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT,
};
use windows_sys::Win32::Networking::WinSock::{
    closesocket, AcceptEx, WSAGetLastError, WSARecv, WSASend, WSASocketW, INVALID_SOCKET, IPPROTO,
    LPWSAOVERLAPPED_COMPLETION_ROUTINE, SEND_RECV_FLAGS, SOCKADDR, SOCKADDR_IN, SOCKET,
    SOCKET_ERROR, WINSOCK_SOCKET_TYPE, WSABUF, WSA_FLAG_OVERLAPPED, WSA_IO_PENDING,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
};

#[cfg(test)]
mod tests;

#[repr(C)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct SocketContext {
    pub(crate) domain: c_int,
    pub(crate) ty: WINSOCK_SOCKET_TYPE,
    pub(crate) protocol: IPPROTO,
}

pub(crate) static SOCKET_CONTEXT: Lazy<DashMap<SOCKET, SocketContext>> =
    Lazy::new(Default::default);

/// The overlapped struct we actually used for IOCP.
#[repr(C)]
#[derive(educe::Educe)]
#[educe(Debug)]
pub(crate) struct Overlapped {
    /// The base [`OVERLAPPED`].
    #[educe(Debug(ignore))]
    pub base: OVERLAPPED,
    pub token: usize,
    pub from_fd: SOCKET,
    pub socket: SOCKET,
    pub syscall: Syscall,
    pub bytes_transferred: u32,
}

impl_display_by_debug!(Overlapped);

#[repr(C)]
#[derive(Debug)]
pub(crate) struct Operator<'o> {
    cpu: usize,
    iocp: HANDLE,
    entering: AtomicBool,
    handles: DashSet<HANDLE>,
    phantom_data: PhantomData<&'o HANDLE>,
}

impl Operator<'_> {
    pub(crate) fn new(cpu: usize) -> std::io::Result<Self> {
        let iocp =
            unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 0) };
        if iocp.is_null() {
            return Err(Error::last_os_error());
        }
        Ok(Self {
            cpu,
            iocp,
            entering: AtomicBool::new(false),
            handles: DashSet::default(),
            phantom_data: PhantomData,
        })
    }

    /// Associates a new `HANDLE` to this I/O completion port.
    ///
    /// This function will associate the given handle to this port with the
    /// given `token` to be returned in status messages whenever it receives a
    /// notification.
    ///
    /// Any object which is convertible to a `HANDLE` via the `AsRawHandle`
    /// trait can be provided to this function, such as `std::fs::File` and
    /// friends.
    fn add_handle(&self, handle: HANDLE) -> std::io::Result<()> {
        if self.handles.contains(&handle) {
            return Ok(());
        }
        let ret = unsafe { CreateIoCompletionPort(handle, self.iocp, self.cpu, 0) };
        if ret.is_null() {
            return Err(Error::new(
                ErrorKind::Other,
                format!("bind handle:{} to IOCP failed", handle as usize),
            ));
        }
        debug_assert_eq!(ret, self.iocp);
        Ok(())
    }

    pub(crate) fn select(
        &self,
        timeout: Option<Duration>,
        want: usize,
    ) -> std::io::Result<(usize, Vec<Overlapped>, Option<Duration>)> {
        if self
            .entering
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok((0, Vec::new(), timeout));
        }
        let result = self.do_select(timeout, want);
        self.entering.store(false, Ordering::Release);
        result
    }

    fn do_select(
        &self,
        timeout: Option<Duration>,
        want: usize,
    ) -> std::io::Result<(usize, Vec<Overlapped>, Option<Duration>)> {
        let start_time = Instant::now();
        let timeout_time = timeout.map_or(u64::MAX, get_timeout_time);
        let mut cq = Vec::new();
        loop {
            let mut bytes = 0;
            let mut overlapped: Overlapped = unsafe { std::mem::zeroed() };
            let ret = unsafe {
                GetQueuedCompletionStatus(
                    self.iocp,
                    &mut bytes,
                    &mut self.cpu.clone(),
                    std::ptr::from_mut(&mut overlapped).cast(),
                    1,
                )
            };
            eprintln!("IOCP got Overlapped:{overlapped}");
            if ret == FALSE {
                if timeout_time.saturating_sub(now()) == 0 {
                    break;
                }
                let err = Error::last_os_error().raw_os_error();
                if Some(WAIT_TIMEOUT as i32) == err {
                    continue;
                }
                if Some(ERROR_NETNAME_DELETED as i32) == err {
                    _ = unsafe { closesocket(overlapped.socket) };
                    continue;
                }
            }
            overlapped.bytes_transferred = bytes;
            cq.push(overlapped);
            if cq.len() >= want {
                break;
            }
        }
        let cost = Instant::now().saturating_duration_since(start_time);
        Ok((cq.len(), cq, timeout.map(|t| t.saturating_sub(cost))))
    }

    pub(crate) fn accept(
        &self,
        user_data: usize,
        fd: SOCKET,
        _address: *mut SOCKADDR,
        _address_len: *mut c_int,
    ) -> std::io::Result<()> {
        if let Some(context) = SOCKET_CONTEXT.get(&fd) {
            self.add_handle(fd as HANDLE)?;
            let ctx = context.value();
            unsafe {
                let socket = WSASocketW(
                    ctx.domain,
                    ctx.ty,
                    ctx.protocol,
                    std::ptr::null(),
                    0,
                    WSA_FLAG_OVERLAPPED,
                );
                if INVALID_SOCKET == socket {
                    return Err(Error::new(
                        ErrorKind::WouldBlock,
                        "add accept operation failed",
                    ));
                }
                let size = size_of::<SOCKADDR_IN>()
                    .saturating_add(16)
                    .try_into()
                    .expect("size overflow");
                let mut overlapped: Overlapped = std::mem::zeroed();
                overlapped.from_fd = fd;
                overlapped.socket = socket;
                overlapped.token = user_data;
                overlapped.syscall = Syscall::accept;
                let mut buf: Vec<u8> = Vec::with_capacity(size as usize * 2);
                while AcceptEx(
                    fd,
                    socket,
                    buf.as_mut_ptr().cast(),
                    0,
                    size,
                    size,
                    std::ptr::null_mut(),
                    std::ptr::from_mut(&mut overlapped).cast(),
                ) == FALSE
                {
                    if WSA_IO_PENDING == WSAGetLastError() {
                        break;
                    }
                }
                eprintln!("add accept operation Overlapped:{overlapped}");
            }
            return Ok(());
        }
        Err(Error::new(
            ErrorKind::Unsupported,
            "accept only supportd for socket",
        ))
    }

    pub(crate) fn recv(
        &self,
        user_data: usize,
        fd: SOCKET,
        buf: PSTR,
        len: c_int,
        flags: SEND_RECV_FLAGS,
    ) -> std::io::Result<()> {
        self.add_handle(fd as HANDLE)?;
        unsafe {
            let mut overlapped: Overlapped = std::mem::zeroed();
            overlapped.from_fd = fd;
            overlapped.token = user_data;
            overlapped.syscall = Syscall::recv;
            let buf = [WSABUF {
                len: len.try_into().expect("len overflow"),
                buf: buf.cast(),
            }];
            if WSARecv(
                fd,
                buf.as_ptr(),
                buf.len().try_into().expect("len overflow"),
                std::ptr::null_mut(),
                &mut u32::try_from(flags).expect("overflow"),
                std::ptr::from_mut(&mut overlapped).cast(),
                None,
            ) == SOCKET_ERROR
                && WSA_IO_PENDING != WSAGetLastError()
            {
                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    "add recv operation failed",
                ));
            }
            eprintln!("add recv operation Overlapped:{overlapped}");
        }
        Ok(())
    }

    pub(crate) fn WSARecv(
        &self,
        user_data: usize,
        fd: SOCKET,
        buf: *const WSABUF,
        dwbuffercount: c_uint,
        lpnumberofbytesrecvd: *mut c_uint,
        lpflags: *mut c_uint,
        lpoverlapped: *mut OVERLAPPED,
        lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
    ) -> std::io::Result<()> {
        assert!(
            lpoverlapped.is_null(),
            "the WSARecv in Operator should be called without lpoverlapped! Please report bug to open-coroutine!"
        );
        self.add_handle(fd as HANDLE)?;
        unsafe {
            let mut overlapped: Overlapped = std::mem::zeroed();
            overlapped.from_fd = fd;
            overlapped.token = user_data;
            overlapped.syscall = Syscall::WSARecv;
            if WSARecv(
                fd,
                buf,
                dwbuffercount,
                lpnumberofbytesrecvd,
                lpflags,
                std::ptr::from_mut(&mut overlapped).cast(),
                lpcompletionroutine,
            ) == SOCKET_ERROR
                && WSA_IO_PENDING != WSAGetLastError()
            {
                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    "add WSARecv operation failed",
                ));
            }
            eprintln!("add WSARecv operation Overlapped:{overlapped}");
        }
        Ok(())
    }

    pub(crate) fn send(
        &self,
        user_data: usize,
        fd: SOCKET,
        buf: PCSTR,
        len: c_int,
        flags: SEND_RECV_FLAGS,
    ) -> std::io::Result<()> {
        self.add_handle(fd as HANDLE)?;
        unsafe {
            let mut overlapped: Overlapped = std::mem::zeroed();
            overlapped.from_fd = fd;
            overlapped.token = user_data;
            overlapped.syscall = Syscall::send;
            let buf = [WSABUF {
                len: len.try_into().expect("len overflow"),
                buf: buf.cast_mut(),
            }];
            if WSASend(
                fd,
                buf.as_ptr(),
                buf.len().try_into().expect("len overflow"),
                std::ptr::null_mut(),
                u32::try_from(flags).expect("overflow"),
                std::ptr::from_mut(&mut overlapped).cast(),
                None,
            ) == SOCKET_ERROR
                && WSA_IO_PENDING != WSAGetLastError()
            {
                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    "add send operation failed",
                ));
            }
            eprintln!("add send operation Overlapped:{overlapped}");
        }
        Ok(())
    }

    pub(crate) fn write(
        &self,
        user_data: usize,
        fd: c_int,
        buf: *const c_void,
        count: c_uint,
    ) -> std::io::Result<()> {
        self.add_handle(fd as HANDLE)?;
        unsafe {
            if SOCKET_CONTEXT.get(&(fd as SOCKET)).is_some() {
                let mut overlapped: Overlapped = std::mem::zeroed();
                overlapped.from_fd = fd as _;
                overlapped.token = user_data;
                overlapped.syscall = Syscall::write;
                let buf = [WSABUF {
                    len: count.try_into().expect("len overflow"),
                    buf: buf.cast_mut().cast(),
                }];
                if WSASend(
                    fd as _,
                    buf.as_ptr(),
                    buf.len().try_into().expect("len overflow"),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::from_mut(&mut overlapped).cast(),
                    None,
                ) == SOCKET_ERROR
                    && WSA_IO_PENDING != WSAGetLastError()
                {
                    return Err(Error::new(
                        ErrorKind::WouldBlock,
                        "add send operation failed",
                    ));
                }
                eprintln!("add send operation Overlapped:{overlapped}");
                return Ok(());
            }
        }
        todo!()
    }

    pub(crate) fn WSASend(
        &self,
        user_data: usize,
        fd: SOCKET,
        buf: *const WSABUF,
        dwbuffercount: c_uint,
        lpnumberofbytesrecvd: *mut c_uint,
        dwflags: c_uint,
        lpoverlapped: *mut OVERLAPPED,
        lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
    ) -> std::io::Result<()> {
        assert!(
            lpoverlapped.is_null(),
            "the WSASend in Operator should be called without lpoverlapped! Please report bug to open-coroutine!"
        );
        self.add_handle(fd as HANDLE)?;
        unsafe {
            let mut overlapped: Overlapped = std::mem::zeroed();
            overlapped.from_fd = fd;
            overlapped.token = user_data;
            overlapped.syscall = Syscall::WSASend;
            if WSASend(
                fd,
                buf,
                dwbuffercount,
                lpnumberofbytesrecvd,
                dwflags,
                std::ptr::from_mut(&mut overlapped).cast(),
                lpcompletionroutine,
            ) == SOCKET_ERROR
                && WSA_IO_PENDING != WSAGetLastError()
            {
                return Err(Error::new(
                    ErrorKind::WouldBlock,
                    "add WSASend operation failed",
                ));
            }
            eprintln!("add WSASend operation Overlapped:{overlapped}");
        }
        Ok(())
    }
}