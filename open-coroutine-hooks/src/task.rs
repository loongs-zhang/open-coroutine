use open_coroutine_core::common::JoinHandle;
use open_coroutine_core::net::core::EventLoops;
use open_coroutine_core::net::event_loop::join::JoinHandleImpl;
use std::ffi::{c_long, c_void};
use std::time::Duration;

///创建task
#[no_mangle]
pub extern "C" fn task_crate(
    f: extern "C" fn(usize) -> usize,
    param: usize,
    stack_size: usize,
) -> JoinHandleImpl<'static> {
    let _stack_size = if stack_size > 0 {
        Some(stack_size)
    } else {
        None
    };
    EventLoops::submit(None, move |param| Some(f(param.unwrap_or(0))), Some(param))
}

///等待task完成
#[no_mangle]
pub extern "C" fn task_join(handle: JoinHandleImpl<'static>) -> c_long {
    match handle.join() {
        Ok(ptr) => match ptr {
            Ok(ptr) => match ptr {
                Some(ptr) => ptr as *mut c_void as c_long,
                None => 0,
            },
            Err(_) => -1,
        },
        Err(_) => -1,
    }
}

///等待task完成
#[no_mangle]
pub extern "C" fn task_timeout_join(handle: &JoinHandleImpl<'static>, ns_time: u64) -> c_long {
    match handle.timeout_join(Duration::from_nanos(ns_time)) {
        Ok(ptr) => match ptr {
            Ok(ptr) => match ptr {
                Some(ptr) => ptr as *mut c_void as c_long,
                None => 0,
            },
            Err(_) => -1,
        },
        Err(_) => -1,
    }
}
