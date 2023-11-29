pub mod config;

mod selector;

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
#[cfg(all(target_os = "linux", feature = "io_uring"))]
pub mod io_uring;

#[allow(
    dead_code,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    trivial_numeric_casts
)]
pub mod event_loop;
