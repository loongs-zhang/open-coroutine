use crate::net::event_loop::EventLoopImpl;
use crate::pool::join::JoinHandle;
use crate::pool::Pool;
use std::ffi::{c_char, CStr, CString};
use std::io::{Error, ErrorKind};
use std::time::Duration;

#[allow(missing_docs)]
#[repr(C)]
#[derive(Debug)]
pub struct JoinHandleImpl<'e>(*const EventLoopImpl<'e>, *const c_char);

impl<'e> JoinHandleImpl<'e> {
    #[allow(box_pointers)]
    pub(crate) fn new(event_loop: *const EventLoopImpl<'e>, name: &str) -> Self {
        let boxed: &'static mut CString = Box::leak(Box::from(
            CString::new(name).expect("init JoinHandle failed!"),
        ));
        let cstr: &'static CStr = boxed.as_c_str();
        JoinHandleImpl(event_loop, cstr.as_ptr())
    }

    /// create a error instance.
    #[must_use]
    pub fn error() -> Self {
        Self::new(std::ptr::null(), "")
    }
}

impl JoinHandle for JoinHandleImpl<'_> {
    fn get_name(&self) -> std::io::Result<&str> {
        unsafe { CStr::from_ptr(self.1) }
            .to_str()
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "Invalid task name"))
    }

    fn timeout_at_join(&self, timeout_time: u64) -> std::io::Result<Result<Option<usize>, &str>> {
        let name = self.get_name()?;
        if name.is_empty() {
            return Err(Error::new(ErrorKind::InvalidInput, "Invalid task name"));
        }
        let event_loop = unsafe { &*self.0 };
        event_loop
            .wait_result(
                name,
                Duration::from_nanos(timeout_time.saturating_sub(open_coroutine_timer::now())),
            )
            .map(|r| r.expect("result is None !").1)
    }
}
