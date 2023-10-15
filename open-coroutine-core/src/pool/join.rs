use crate::pool::{CoroutinePoolImpl, Pool};
use std::ffi::{c_char, CStr, CString};
use std::io::{Error, ErrorKind};
use std::time::Duration;

/// Task join abstraction.
pub trait JoinHandle {
    /// get the task name.
    ///
    /// # Errors
    /// if the task name is invalid.
    fn get_name(&self) -> std::io::Result<&str>;

    /// join with `Duration`.
    ///
    /// # Errors
    /// see `timeout_at_join`.
    fn timeout_join(&self, dur: Duration) -> std::io::Result<Result<Option<usize>, &str>> {
        self.timeout_at_join(open_coroutine_timer::get_timeout_time(dur))
    }

    /// join.
    ///
    /// # Errors
    /// see `timeout_at_join`.
    fn join(&self) -> std::io::Result<Result<Option<usize>, &str>> {
        self.timeout_at_join(u64::MAX)
    }

    /// join with timeout.
    ///
    /// # Errors
    /// if join failed.
    fn timeout_at_join(&self, timeout_time: u64) -> std::io::Result<Result<Option<usize>, &str>>;
}

#[allow(missing_docs)]
#[repr(C)]
#[derive(Debug)]
pub struct JoinHandleImpl<'p>(*const CoroutinePoolImpl<'p>, *const c_char);

impl<'p> JoinHandleImpl<'p> {
    #[allow(box_pointers)]
    pub(crate) fn new(pool: *const CoroutinePoolImpl<'p>, name: &str) -> Self {
        let boxed: &'static mut CString = Box::leak(Box::from(
            CString::new(name).expect("init JoinHandle failed!"),
        ));
        let cstr: &'static CStr = boxed.as_c_str();
        JoinHandleImpl(pool, cstr.as_ptr())
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
        let pool = unsafe { &*self.0 };
        pool.wait_result(
            name,
            Duration::from_nanos(timeout_time.saturating_sub(open_coroutine_timer::now())),
        )
        .map(|r| r.expect("result is None !").1)
    }
}

#[allow(box_pointers)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::{CoroutinePool, Pool};
    use std::sync::{Arc, Condvar, Mutex};

    #[test]
    fn join_test() -> std::io::Result<()> {
        let pair = Arc::new((Mutex::new(true), Condvar::new()));
        let pair2 = Arc::clone(&pair);
        let handler = std::thread::Builder::new()
            .name("test_join".to_string())
            .spawn(move || {
                let pool = CoroutinePoolImpl::default();
                _ = pool.change_blocker(crate::blocker::DelayBlocker::default());
                let handle1 = pool.submit(
                    None,
                    |_| {
                        println!("[coroutine1] launched");
                        Some(3)
                    },
                    None,
                );
                let handle2 = pool.submit(
                    None,
                    |_| {
                        println!("[coroutine2] launched");
                        Some(4)
                    },
                    None,
                );
                pool.try_schedule().unwrap();
                assert_eq!(handle1.join().unwrap().unwrap().unwrap(), 3);
                assert_eq!(handle2.join().unwrap().unwrap().unwrap(), 4);

                let (lock, cvar) = &*pair2;
                let mut pending = lock.lock().unwrap();
                *pending = false;
                // notify the condvar that the value has changed.
                cvar.notify_one();
            })
            .expect("failed to spawn thread");

        // wait for the thread to start up
        let (lock, cvar) = &*pair;
        let result = cvar
            .wait_timeout_while(
                lock.lock().unwrap(),
                Duration::from_millis(3000),
                |&mut pending| pending,
            )
            .unwrap();
        if result.1.timed_out() {
            Err(Error::new(ErrorKind::TimedOut, "join failed"))
        } else {
            handler.join().unwrap();
            Ok(())
        }
    }

    #[test]
    fn timed_join_test() -> std::io::Result<()> {
        let pair = Arc::new((Mutex::new(true), Condvar::new()));
        let pair2 = Arc::clone(&pair);
        let handler = std::thread::Builder::new()
            .name("test_timed_join".to_string())
            .spawn(move || {
                let pool = CoroutinePoolImpl::default();
                _ = pool.change_blocker(crate::blocker::DelayBlocker::default());
                let handle = pool.submit(
                    None,
                    |_| {
                        println!("[coroutine3] launched");
                        Some(5)
                    },
                    None,
                );
                let error = handle.timeout_join(Duration::from_nanos(0)).unwrap_err();
                assert_eq!(error.kind(), ErrorKind::TimedOut);
                pool.try_schedule().unwrap();
                assert_eq!(
                    handle
                        .timeout_join(Duration::from_secs(1))
                        .unwrap()
                        .unwrap()
                        .unwrap(),
                    5
                );

                let (lock, cvar) = &*pair2;
                let mut pending = lock.lock().unwrap();
                *pending = false;
                // notify the condvar that the value has changed.
                cvar.notify_one();
            })
            .expect("failed to spawn thread");

        // wait for the thread to start up
        let (lock, cvar) = &*pair;
        let result = cvar
            .wait_timeout_while(
                lock.lock().unwrap(),
                Duration::from_millis(3000),
                |&mut pending| pending,
            )
            .unwrap();
        if result.1.timed_out() {
            Err(Error::new(ErrorKind::TimedOut, "timed join failed"))
        } else {
            handler.join().unwrap();
            Ok(())
        }
    }
}
