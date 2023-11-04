/// Event driven abstraction and impl.
pub mod selector;

/// `io_uring` abstraction and impl.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
#[cfg(all(target_os = "linux", feature = "io_uring"))]
pub mod operator;

#[allow(missing_docs)]
#[cfg(all(target_os = "linux", feature = "io_uring"))]
pub mod event_loop;

/// Global config abstraction and impl.
pub mod config;
