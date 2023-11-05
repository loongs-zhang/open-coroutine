use libc::{c_int, c_uint, timespec};
use once_cell::sync::Lazy;

static SLEEP: Lazy<extern "C" fn(c_uint) -> c_uint> = init_hook!("sleep");

#[no_mangle]
pub extern "C" fn sleep(secs: c_uint) -> c_uint {
    open_coroutine_core::syscall::sleep(Some(Lazy::force(&SLEEP)), secs)
}

static USLEEP: Lazy<extern "C" fn(c_uint) -> c_int> = init_hook!("usleep");

#[no_mangle]
pub extern "C" fn usleep(microseconds: c_uint) -> c_int {
    open_coroutine_core::syscall::usleep(Some(Lazy::force(&USLEEP)), microseconds)
}

static NANOSLEEP: Lazy<extern "C" fn(*const timespec, *mut timespec) -> c_int> =
    init_hook!("nanosleep");

#[no_mangle]
pub extern "C" fn nanosleep(rqtp: *const timespec, rmtp: *mut timespec) -> c_int {
    open_coroutine_core::syscall::nanosleep(Some(Lazy::force(&NANOSLEEP)), rqtp, rmtp)
}
