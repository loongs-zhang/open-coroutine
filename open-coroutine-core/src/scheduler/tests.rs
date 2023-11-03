use super::*;
use crate::coroutine::suspender::{SimpleDelaySuspender, SimpleSuspender};
use std::time::Duration;

#[test]
fn test_simple() -> std::io::Result<()> {
    let mut scheduler = SchedulerImpl::default();
    scheduler.submit_coroutine(|_, _| println!("1"), None)?;
    scheduler.submit_coroutine(|_, _| println!("2"), None)?;
    scheduler.try_schedule()
}

#[test]
fn test_backtrace() -> std::io::Result<()> {
    let mut scheduler = SchedulerImpl::default();
    scheduler.submit_coroutine(|_, _| (), None)?;
    scheduler.submit_coroutine(|_, _| println!("{:?}", backtrace::Backtrace::new()), None)?;
    scheduler.try_schedule()
}

#[test]
fn with_suspend() -> std::io::Result<()> {
    let mut scheduler = SchedulerImpl::default();
    scheduler.submit_coroutine(
        |suspender, _| {
            println!("[coroutine1] suspend");
            suspender.suspend();
            println!("[coroutine1] back");
        },
        None,
    )?;
    scheduler.submit_coroutine(
        |suspender, _| {
            println!("[coroutine2] suspend");
            suspender.suspend();
            println!("[coroutine2] back");
        },
        None,
    )?;
    scheduler.try_schedule()
}

#[test]
fn with_delay() -> std::io::Result<()> {
    let mut scheduler = SchedulerImpl::default();
    scheduler.submit_coroutine(
        |suspender, _| {
            println!("[coroutine] delay");
            suspender.delay(Duration::from_millis(100));
            println!("[coroutine] back");
        },
        None,
    )?;
    scheduler.try_schedule()?;
    std::thread::sleep(Duration::from_millis(100));
    scheduler.try_schedule()
}

#[test]
fn test_current() -> std::io::Result<()> {
    let parent_name = "parent";
    let mut scheduler = SchedulerImpl::new(
        String::from(parent_name),
        crate::constants::DEFAULT_STACK_SIZE,
    );
    scheduler.submit_coroutine(
        |_, _| {
            assert!(SchedulableCoroutine::current().is_some());
            assert!(SchedulableSuspender::current().is_some());
            assert_eq!(parent_name, SchedulerImpl::current().unwrap().get_name());
            assert_eq!(parent_name, SchedulerImpl::current().unwrap().get_name());

            let child_name = "child";
            let mut scheduler = SchedulerImpl::new(
                String::from(child_name),
                crate::constants::DEFAULT_STACK_SIZE,
            );
            scheduler
                .submit_coroutine(
                    |_, _| {
                        assert!(SchedulableCoroutine::current().is_some());
                        assert!(SchedulableSuspender::current().is_some());
                        assert_eq!(child_name, SchedulerImpl::current().unwrap().get_name());
                        assert_eq!(child_name, SchedulerImpl::current().unwrap().get_name());
                    },
                    None,
                )
                .unwrap();
            scheduler.try_schedule().unwrap();

            assert_eq!(parent_name, SchedulerImpl::current().unwrap().get_name());
            assert_eq!(parent_name, SchedulerImpl::current().unwrap().get_name());
        },
        None,
    )?;
    scheduler.try_schedule()
}

#[test]
fn test_trap() -> std::io::Result<()> {
    let mut scheduler = SchedulerImpl::default();
    scheduler.submit_coroutine(
        |_, _| {
            println!("Before trap");
            unsafe { std::ptr::write_volatile(1 as *mut u8, 0) };
            println!("After trap");
        },
        None,
    )?;
    scheduler.submit_coroutine(|_, _| println!("200"), None)?;
    scheduler.try_schedule()
}

#[cfg(not(debug_assertions))]
#[test]
fn test_invalid_memory_reference() -> std::io::Result<()> {
    let mut scheduler = SchedulerImpl::default();
    scheduler.submit_coroutine(
        |_, _| {
            println!("Before invalid memory reference");
            // 没有加--release运行，会收到SIGABRT信号，不好处理，直接禁用测试
            unsafe { _ = &*((1usize as *mut c_void).cast::<SchedulableCoroutine>()) };
            println!("After invalid memory reference");
        },
        None,
    )?;
    scheduler.submit_coroutine(|_, _| println!("200"), None)?;
    scheduler.try_schedule()
}
