fn main() -> std::io::Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(all(unix, feature = "preemptive-schedule"))] {
            use open_coroutine_core::scheduler::{Scheduler, SchedulerImpl};
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::{Arc, Condvar, Mutex};

            static TEST_FLAG1: AtomicBool = AtomicBool::new(true);
            static TEST_FLAG2: AtomicBool = AtomicBool::new(true);
            let pair = Arc::new((Mutex::new(true), Condvar::new()));
            let pair2 = Arc::clone(&pair);
            let _: std::thread::JoinHandle<std::io::Result<()>> = std::thread::Builder::new()
                .name("test_preemptive_schedule".to_string())
                .spawn(move || {
                    let scheduler = SchedulerImpl::default();
                    _ = scheduler.submit_co(
                        |_, _| {
                            while TEST_FLAG1.load(Ordering::Acquire) {
                                _ = unsafe { libc::usleep(10_000) };
                            }
                            None
                        },
                        None,
                    );
                    _ = scheduler.submit_co(
                        |_, _| {
                            while TEST_FLAG2.load(Ordering::Acquire) {
                                _ = unsafe { libc::usleep(10_000) };
                            }
                            TEST_FLAG1.store(false, Ordering::Release);
                            None
                        },
                        None,
                    );
                    _ = scheduler.submit_co(
                        |_, _| {
                            TEST_FLAG2.store(false, Ordering::Release);
                            None
                        },
                        None,
                    );
                    scheduler.try_schedule()?;

                    let (lock, cvar) = &*pair2;
                    let mut pending = lock.lock().unwrap();
                    *pending = false;
                    // notify the condvar that the value has changed.
                    cvar.notify_one();
                    Ok(())
                })
                .expect("failed to spawn thread");

            // wait for the thread to start up
            let (lock, cvar) = &*pair;
            _ = cvar
                .wait_timeout_while(
                    lock.lock().unwrap(),
                    std::time::Duration::from_millis(3000),
                    |&mut pending| pending,
                )
                .unwrap();
            if TEST_FLAG1.load(Ordering::Acquire) {
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "preemptive schedule failed",
                ))
            } else {
                Ok(())
            }
        } else {
            println!("please enable preemptive-schedule feature");
            Ok(())
        }
    }
}
