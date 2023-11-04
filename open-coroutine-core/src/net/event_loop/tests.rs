use super::*;
use crate::pool::SubmittableTaskPool;

#[test]
fn test_simple() -> std::io::Result<()> {
    let event_loop = EventLoopImpl::default();
    event_loop.set_max_size(1);
    _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
    _ = event_loop.submit(
        None,
        |_| {
            println!("2");
            Some(2)
        },
        None,
    );
    event_loop.stop(Duration::from_secs(3))
}

#[test]
fn test_wait() -> std::io::Result<()> {
    let event_loop = EventLoopImpl::default();
    event_loop.set_max_size(1);
    let task_name = uuid::Uuid::new_v4().to_string();
    _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
    let result = event_loop.submit_and_wait(
        Some(task_name.clone()),
        |_| {
            println!("2");
            Some(2)
        },
        None,
        Duration::from_millis(100),
    );
    assert_eq!(Some((task_name, Ok(Some(2)))), result.unwrap());
    event_loop.stop(Duration::from_secs(3))
}

#[test]
fn test_simple_auto() -> std::io::Result<()> {
    let event_loop = EventLoopImpl::default().start()?;
    event_loop.set_max_size(1);
    _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
    _ = event_loop.submit(
        None,
        |_| {
            println!("2");
            Some(2)
        },
        None,
    );
    event_loop.stop(Duration::from_secs(3))
}

#[test]
fn test_wait_auto() -> std::io::Result<()> {
    let event_loop = EventLoopImpl::default().start()?;
    event_loop.set_max_size(1);
    let task_name = uuid::Uuid::new_v4().to_string();
    _ = event_loop.submit(None, |_| panic!("test panic, just ignore it"), None);
    let result = event_loop.submit_and_wait(
        Some(task_name.clone()),
        |_| {
            println!("2");
            Some(2)
        },
        None,
        Duration::from_secs(3),
    );
    assert_eq!(Some((task_name, Ok(Some(2)))), result.unwrap());
    event_loop.stop(Duration::from_secs(3))
}
