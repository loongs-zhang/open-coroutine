mod event;

mod interest;

mod selector;

use crate::event_loop::event::Events;
use crate::event_loop::interest::Interest;
use crate::event_loop::selector::Selector;
use base_coroutine::{Coroutine, Scheduler};
use once_cell::sync::Lazy;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static mut INDEX: Lazy<AtomicUsize> = Lazy::new(|| AtomicUsize::new(0));

static mut EVENT_LOOPS: Lazy<Box<[EventLoop]>> = Lazy::new(|| {
    (0..num_cpus::get())
        .map(|_| {
            // 由于EVENT_LOOPS的初始化是由单线程完成的
            // 创建线程再获取Scheduler是为了拿到不同的Scheduler
            std::thread::spawn(|| EventLoop::new().expect("init event loop failed!"))
                .join()
                .expect("create scheduler failed !")
        })
        .collect()
});

pub struct EventLoop<'a> {
    selector: Selector,
    scheduler: &'a mut Scheduler,
}

unsafe impl Send for EventLoop<'_> {}

impl<'a> EventLoop<'a> {
    fn new() -> std::io::Result<EventLoop<'a>> {
        Ok(EventLoop {
            selector: Selector::new()?,
            scheduler: Scheduler::current(),
        })
    }

    pub fn next() -> &'static mut EventLoop<'static> {
        unsafe {
            let index = INDEX.fetch_add(1, Ordering::SeqCst);
            if index == usize::MAX {
                INDEX.store(1, Ordering::SeqCst);
            }
            EVENT_LOOPS.get_mut(index % num_cpus::get()).unwrap()
        }
    }

    pub fn next_scheduler() -> &'static mut Scheduler {
        EventLoop::next().scheduler
    }

    pub fn add_read_event(&self, fd: libc::c_int) -> std::io::Result<()> {
        if let Some(co) = Coroutine::<&'static mut c_void, &'static mut c_void>::current() {
            self.selector
                .register(fd, co.get_id(), Interest::READABLE)?;
        }
        Ok(())
    }

    pub fn add_write_event(&self, fd: libc::c_int) -> std::io::Result<()> {
        if let Some(co) = Coroutine::<&'static mut c_void, &'static mut c_void>::current() {
            self.selector
                .register(fd, co.get_id(), Interest::WRITABLE)?;
        }
        Ok(())
    }

    pub fn del_event(&self, fd: libc::c_int) -> std::io::Result<()> {
        self.selector.deregister(fd)?;
        Ok(())
    }

    pub fn wait(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.scheduler.syscall();
        let mut events = Events::with_capacity(1024);
        //默认1s超时
        self.selector
            .select(&mut events, Some(timeout.unwrap_or(Duration::from_secs(1))))?;
        for event in events.iter() {
            unsafe {
                self.scheduler.resume(event.token());
            }
        }
        Ok(())
    }
}
