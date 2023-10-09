use std::fmt::{Debug, Display, Formatter};

/// Enums used to describe pool state
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PoolState {
    ///The pool is created.
    Created,
    ///The pool is running in an additional thread.
    Running,
    ///The pool is stopping, `true` means thread mode.
    Stopping(bool),
    ///The pool is stopped.
    Stopped,
}

impl Display for PoolState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}
