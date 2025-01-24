use once_cell::sync::Lazy;
use std::ffi::{c_int, c_uint};
use windows_sys::Win32::Networking::WinSock::{LPWSAOVERLAPPED_COMPLETION_ROUTINE, SOCKET, WSABUF};
use windows_sys::Win32::System::IO::OVERLAPPED;

#[must_use]
pub extern "system" fn WSASend(
    fn_ptr: Option<
        &extern "system" fn(
            SOCKET,
            *const WSABUF,
            c_uint,
            *mut c_uint,
            c_uint,
            *mut OVERLAPPED,
            LPWSAOVERLAPPED_COMPLETION_ROUTINE,
        ) -> c_int,
    >,
    fd: SOCKET,
    buf: *const WSABUF,
    dwbuffercount: c_uint,
    lpnumberofbytessent: *mut c_uint,
    dwflags: c_uint,
    lpoverlapped: *mut OVERLAPPED,
    lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
) -> c_int {
    cfg_if::cfg_if! {
        if #[cfg(all(windows, feature = "iocp"))] {
            static CHAIN: Lazy<
                WSASendSyscallFacade<IocpWSASendSyscall<NioWSASendSyscall<RawWSASendSyscall>>>
            > = Lazy::new(Default::default);
        } else {
            static CHAIN: Lazy<WSASendSyscallFacade<NioWSASendSyscall<RawWSASendSyscall>>> =
                Lazy::new(Default::default);
        }
    }
    CHAIN.WSASend(
        fn_ptr,
        fd,
        buf,
        dwbuffercount,
        lpnumberofbytessent,
        dwflags,
        lpoverlapped,
        lpcompletionroutine,
    )
}

trait WSASendSyscall {
    extern "system" fn WSASend(
        &self,
        fn_ptr: Option<
            &extern "system" fn(
                SOCKET,
                *const WSABUF,
                c_uint,
                *mut c_uint,
                c_uint,
                *mut OVERLAPPED,
                LPWSAOVERLAPPED_COMPLETION_ROUTINE,
            ) -> c_int,
        >,
        fd: SOCKET,
        buf: *const WSABUF,
        dwbuffercount: c_uint,
        lpnumberofbytessent: *mut c_uint,
        dwflags: c_uint,
        lpoverlapped: *mut OVERLAPPED,
        lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
    ) -> c_int;
}

impl_facade!(WSASendSyscallFacade, WSASendSyscall,
    WSASend(
        fd: SOCKET,
        buf: *const WSABUF,
        dwbuffercount: c_uint,
        lpnumberofbytessent: *mut c_uint,
        dwflags: c_uint,
        lpoverlapped: *mut OVERLAPPED,
        lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE
    ) -> c_int
);

#[cfg(all(windows, feature = "iocp"))]
#[repr(C)]
#[derive(Debug, Default)]
struct IocpWSASendSyscall<I: WSASendSyscall> {
    inner: I,
}

#[cfg(all(windows, feature = "iocp"))]
impl<I: WSASendSyscall> WSASendSyscall for IocpWSASendSyscall<I> {
    extern "system" fn WSASend(
        &self,
        fn_ptr: Option<
            &extern "system" fn(
                SOCKET,
                *const WSABUF,
                c_uint,
                *mut c_uint,
                c_uint,
                *mut OVERLAPPED,
                LPWSAOVERLAPPED_COMPLETION_ROUTINE,
            ) -> c_int,
        >,
        fd: SOCKET,
        buf: *const WSABUF,
        dwbuffercount: c_uint,
        lpnumberofbytessent: *mut c_uint,
        dwflags: c_uint,
        lpoverlapped: *mut OVERLAPPED,
        lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
    ) -> c_int {
        use crate::common::constants::{CoroutineState, SyscallState};
        use crate::net::EventLoops;
        use crate::scheduler::{SchedulableCoroutine, SchedulableSuspender};
        use windows_sys::Win32::Networking::WinSock::{SOCKET_ERROR, WSAEWOULDBLOCK};

        if !lpoverlapped.is_null() {
            return RawWSASendSyscall::default().WSASend(
                fn_ptr,
                fd,
                buf,
                dwbuffercount,
                lpnumberofbytessent,
                dwflags,
                lpoverlapped,
                lpcompletionroutine,
            );
        }
        if let Ok(arc) = EventLoops::WSASend(fd, buf, dwbuffercount, lpnumberofbytessent, dwflags, lpoverlapped, lpcompletionroutine) {
            if let Some(co) = SchedulableCoroutine::current() {
                if let CoroutineState::Syscall((), syscall, SyscallState::Executing) = co.state()
                {
                    let new_state = SyscallState::Suspend(crate::syscall::send_time_limit(fd));
                    if co.syscall((), syscall, new_state).is_err() {
                        crate::error!(
                            "{} change to syscall {} {} failed !",
                            co.name(), syscall, new_state
                        );
                    }
                }
            }
            if let Some(suspender) = SchedulableSuspender::current() {
                suspender.suspend();
                //回来的时候，系统调用已经执行完了
            }
            if let Some(co) = SchedulableCoroutine::current() {
                if let CoroutineState::Syscall((), syscall, SyscallState::Callback) = co.state()
                {
                    let new_state = SyscallState::Executing;
                    if co.syscall((), syscall, new_state).is_err() {
                        crate::error!(
                            "{} change to syscall {} {} failed !",
                            co.name(), syscall, new_state
                        );
                    }
                }
            }
            let (lock, cvar) = &*arc;
            let syscall_result: c_int = cvar
                .wait_while(lock.lock().expect("lock failed"),
                            |&mut result| result.is_none()
                )
                .expect("lock failed")
                .expect("no syscall result")
                .try_into()
                .expect("IOCP syscall result overflow");
            if syscall_result < 0 {
                let errno = (-syscall_result).try_into().expect("IOCP errno overflow");
                crate::syscall::set_errno(errno);
                syscall_result = -1;
            }
            return syscall_result;
        }
        self.inner.WSASend(
            fn_ptr,
            fd,
            buf,
            dwbuffercount,
            lpnumberofbytessent,
            dwflags,
            lpoverlapped,
            lpcompletionroutine,
        )
    }
}

impl_nio_write_iovec!(NioWSASendSyscall, WSASendSyscall,
    WSASend(
        fd: SOCKET,
        buf: *const WSABUF,
        dwbuffercount: c_uint,
        lpnumberofbytessent: *mut c_uint,
        dwflags: c_uint,
        lpoverlapped: *mut OVERLAPPED,
        lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE
    ) -> c_int
);

impl_raw!(RawWSASendSyscall, WSASendSyscall, windows_sys::Win32::Networking::WinSock,
    WSASend(
        fd: SOCKET,
        buf: *const WSABUF,
        dwbuffercount: c_uint,
        lpnumberofbytessent: *mut c_uint,
        dwflags: c_uint,
        lpoverlapped: *mut OVERLAPPED,
        lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE
    ) -> c_int
);
