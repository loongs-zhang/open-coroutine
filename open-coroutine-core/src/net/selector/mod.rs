use dashmap::{DashMap, DashSet};
use once_cell::sync::Lazy;
use std::ffi::c_int;
use std::fmt::Debug;
use std::time::Duration;

#[cfg(windows)]
mod polling;

#[cfg(windows)]
pub use polling::{Events, SelectorImpl};

#[cfg(unix)]
mod mio;

#[cfg(unix)]
pub use mio::{Events, SelectorImpl};

/// Event driven abstraction.
pub trait Selector: Debug + Default {
    /// # Errors
    /// if poll failed.
    fn select(&self, events: &mut Events, timeout: Option<Duration>) -> std::io::Result<()>;

    /// # Errors
    /// if add failed.
    fn add_read_event(&self, fd: c_int, token: usize) -> std::io::Result<()>;

    /// # Errors
    /// if add failed.
    fn add_write_event(&self, fd: c_int, token: usize) -> std::io::Result<()>;

    /// # Errors
    /// if delete failed.
    fn del_event(&self, fd: c_int) -> std::io::Result<()>;

    /// # Errors
    /// if delete failed.
    ///
    /// # Panics
    /// if clean failed.
    fn del_read_event(&self, fd: c_int) -> std::io::Result<()>;

    /// # Errors
    /// if delete failed.
    ///
    /// # Panics
    /// if clean failed.
    fn del_write_event(&self, fd: c_int) -> std::io::Result<()>;
}

static TOKEN_FD: Lazy<DashMap<usize, c_int>> = Lazy::new(DashMap::new);

static READABLE_RECORDS: Lazy<DashSet<c_int>> = Lazy::new(DashSet::new);

static READABLE_TOKEN_RECORDS: Lazy<DashMap<c_int, usize>> = Lazy::new(DashMap::new);

static WRITABLE_RECORDS: Lazy<DashSet<c_int>> = Lazy::new(DashSet::new);

static WRITABLE_TOKEN_RECORDS: Lazy<DashMap<c_int, usize>> = Lazy::new(DashMap::new);