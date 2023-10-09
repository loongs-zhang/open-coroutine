use open_coroutine_core::net::event_loop::join::JoinHandleImpl;
use open_coroutine_core::net::EventLoops;
use open_coroutine_core::pool::join::JoinHandle;
use std::ffi::c_void;
use std::time::Duration;

///创建协程
#[no_mangle]
pub extern "C" fn coroutine_crate(
    f: extern "C" fn(usize) -> usize,
    param: usize,
    stack_size: usize,
) -> JoinHandleImpl<'static> {
    let _stack_size = if stack_size > 0 {
        Some(stack_size)
    } else {
        None
    };
    EventLoops::submit(move |param| Some(f(param.unwrap_or(0))), Some(param))
}

///等待协程完成
#[no_mangle]
pub extern "C" fn coroutine_join(handle: JoinHandleImpl<'static>) -> libc::c_long {
    match handle.join() {
        Ok(ptr) => match ptr {
            Ok(ptr) => match ptr {
                Some(ptr) => ptr as *mut c_void as libc::c_long,
                None => 0,
            },
            Err(_) => -1,
        },
        Err(_) => -1,
    }
}

///等待协程完成
#[no_mangle]
pub extern "C" fn coroutine_timeout_join(
    handle: &JoinHandleImpl<'static>,
    ns_time: u64,
) -> libc::c_long {
    match handle.timeout_join(Duration::from_nanos(ns_time)) {
        Ok(ptr) => match ptr {
            Ok(ptr) => match ptr {
                Some(ptr) => ptr as *mut c_void as libc::c_long,
                None => 0,
            },
            Err(_) => -1,
        },
        Err(_) => -1,
    }
}
