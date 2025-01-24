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
            static RAW: Lazy<RawWSASendSyscall> = Lazy::new(Default::default);
            static CHAIN: Lazy<
                WSASendSyscallFacade<IocpWSASendSyscall<NioWSASendSyscall<RawWSASendSyscall>>>
            > = Lazy::new(Default::default);
            if !lpoverlapped.is_null() {
                return RAW.WSASend(
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

impl_iocp_write!(IocpWSASendSyscall, WSASendSyscall,
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
