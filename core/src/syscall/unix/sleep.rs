use crate::net::EventLoops;
use crate::syscall::reset_errno;
use std::ffi::c_uint;
use std::time::Duration;

trait SleepSyscall {
    extern "C" fn sleep(
        &self,
        fn_ptr: Option<&extern "C" fn(c_uint) -> c_uint>,
        secs: c_uint,
    ) -> c_uint;
}

impl_syscall!(SleepSyscallFacade, NioSleepSyscall, sleep(secs: c_uint) -> c_uint);

impl_facade!(SleepSyscallFacade, SleepSyscall, sleep(secs: c_uint) -> c_uint);

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
struct NioSleepSyscall {}

impl SleepSyscall for NioSleepSyscall {
    extern "C" fn sleep(
        &self,
        _: Option<&extern "C" fn(c_uint) -> c_uint>,
        secs: c_uint,
    ) -> c_uint {
        let time = Duration::from_secs(u64::from(secs));
        if let Some(co) = crate::scheduler::SchedulableCoroutine::current() {
            let syscall = crate::common::constants::SyscallName::sleep;
            let new_state = crate::common::constants::SyscallState::Suspend(
                crate::common::get_timeout_time(time),
            );
            if co.syscall((), syscall, new_state).is_err() {
                crate::error!(
                    "{} change to syscall {} {} failed !",
                    co.name(),
                    syscall,
                    new_state
                );
            }
        }
        _ = EventLoops::wait_event(Some(time));
        reset_errno();
        0
    }
}
