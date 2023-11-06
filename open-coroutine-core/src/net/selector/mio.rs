use crate::net::selector::{
    Event, Selector, READABLE_RECORDS, READABLE_TOKEN_RECORDS, TOKEN_FD, WRITABLE_RECORDS,
    WRITABLE_TOKEN_RECORDS,
};
use mio::unix::SourceFd;
use mio::{Interest, Poll, Token};
use std::cell::RefCell;
use std::ffi::c_int;
use std::fmt::Debug;
use std::time::Duration;

pub type Events = mio::Events;

impl Event for &mio::event::Event {
    fn get_token(&self) -> usize {
        self.token().0
    }
}

/// Event driven impl.
#[derive(Debug)]
pub struct SelectorImpl(RefCell<Poll>);

impl SelectorImpl {
    /// # Errors
    /// if create failed.
    pub fn new() -> std::io::Result<Self> {
        Ok(SelectorImpl(RefCell::new(Poll::new()?)))
    }

    fn register(&self, fd: c_int, token: usize, interests: Interest) -> std::io::Result<()> {
        loop {
            if let Ok(poller) = self.0.try_borrow() {
                return poller
                    .registry()
                    .register(&mut SourceFd(&fd), Token(token), interests)
                    .map(|()| {
                        _ = TOKEN_FD.insert(token, fd);
                    });
            }
        }
    }

    fn reregister(&self, fd: c_int, token: usize, interests: Interest) -> std::io::Result<()> {
        loop {
            if let Ok(poller) = self.0.try_borrow() {
                return poller
                    .registry()
                    .reregister(&mut SourceFd(&fd), Token(token), interests)
                    .map(|()| {
                        _ = TOKEN_FD.insert(token, fd);
                    });
            }
        }
    }

    fn deregister(&self, fd: c_int, token: usize) -> std::io::Result<()> {
        loop {
            if let Ok(poller) = self.0.try_borrow() {
                return poller.registry().deregister(&mut SourceFd(&fd)).map(|()| {
                    _ = TOKEN_FD.remove(&token);
                });
            }
        }
    }
}

impl Default for SelectorImpl {
    fn default() -> Self {
        Self::new().expect("create selector failed")
    }
}

impl Selector for SelectorImpl {
    fn select(&self, events: &mut Events, timeout: Option<Duration>) -> std::io::Result<()> {
        if let Ok(mut poller) = self.0.try_borrow_mut() {
            let result = poller.poll(events, timeout);
            for event in &*events {
                let token = event.token().0;
                let fd = TOKEN_FD.remove(&token).map_or(0, |r| r.1);
                if event.is_readable() {
                    _ = READABLE_TOKEN_RECORDS.remove(&fd);
                }
                if event.is_writable() {
                    _ = WRITABLE_TOKEN_RECORDS.remove(&fd);
                }
            }
            return result;
        }
        Ok(())
    }

    fn add_read_event(&self, fd: c_int, token: usize) -> std::io::Result<()> {
        if READABLE_RECORDS.contains(&fd) {
            return Ok(());
        }
        if WRITABLE_RECORDS.contains(&fd) {
            //同时对读写事件感兴趣
            let interests = Interest::READABLE.add(Interest::WRITABLE);
            self.reregister(fd, token, interests)
                .or(self.register(fd, token, interests))
        } else {
            self.register(fd, token, Interest::READABLE)
        }?;
        _ = READABLE_RECORDS.insert(fd);
        _ = READABLE_TOKEN_RECORDS.insert(fd, token);
        Ok(())
    }

    fn add_write_event(&self, fd: c_int, token: usize) -> std::io::Result<()> {
        if WRITABLE_RECORDS.contains(&fd) {
            return Ok(());
        }
        if READABLE_RECORDS.contains(&fd) {
            //同时对读写事件感兴趣
            let interests = Interest::READABLE.add(Interest::WRITABLE);
            self.reregister(fd, token, interests)
                .or(self.register(fd, token, interests))
        } else {
            self.register(fd, token, Interest::WRITABLE)
        }?;
        _ = WRITABLE_RECORDS.insert(fd);
        _ = WRITABLE_TOKEN_RECORDS.insert(fd, token);
        Ok(())
    }

    fn del_event(&self, fd: c_int) -> std::io::Result<()> {
        if READABLE_RECORDS.contains(&fd) || WRITABLE_RECORDS.contains(&fd) {
            let token = READABLE_TOKEN_RECORDS
                .remove(&fd)
                .or(WRITABLE_TOKEN_RECORDS.remove(&fd))
                .map_or(0, |r| r.1);
            self.deregister(fd, token)?;
            _ = READABLE_RECORDS.remove(&fd);
            _ = WRITABLE_RECORDS.remove(&fd);
        }
        Ok(())
    }

    fn del_read_event(&self, fd: c_int) -> std::io::Result<()> {
        if READABLE_RECORDS.contains(&fd) {
            if WRITABLE_RECORDS.contains(&fd) {
                //写事件不能删
                let token = WRITABLE_TOKEN_RECORDS.get(&fd).map_or(0, |r| *r.value());
                self.reregister(fd, token, Interest::WRITABLE)?;
                assert!(
                    READABLE_RECORDS.remove(&fd).is_some(),
                    "Clean READABLE_RECORDS failed !"
                );
                assert!(
                    READABLE_TOKEN_RECORDS.remove(&fd).is_some(),
                    "Clean READABLE_TOKEN_RECORDS failed !"
                );
            } else {
                self.del_event(fd)?;
            }
        }
        Ok(())
    }

    fn del_write_event(&self, fd: c_int) -> std::io::Result<()> {
        if WRITABLE_RECORDS.contains(&fd) {
            if READABLE_RECORDS.contains(&fd) {
                //读事件不能删
                let token = READABLE_TOKEN_RECORDS.get(&fd).map_or(0, |r| *r.value());
                self.reregister(fd, token, Interest::READABLE)?;
                assert!(
                    WRITABLE_RECORDS.remove(&fd).is_some(),
                    "Clean WRITABLE_RECORDS failed !"
                );
                assert!(
                    WRITABLE_TOKEN_RECORDS.remove(&fd).is_some(),
                    "Clean WRITABLE_TOKEN_RECORDS failed !"
                );
            } else {
                self.del_event(fd)?;
            }
        }
        Ok(())
    }
}
