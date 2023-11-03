/// init log framework.
#[allow(box_pointers)]
#[cfg(feature = "logs")]
pub fn init() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOG_INITED: AtomicBool = AtomicBool::new(false);
    if LOG_INITED
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        let mut builder = simplelog::ConfigBuilder::new();
        let result = builder.set_time_format_rfc2822().set_time_offset_to_local();
        let config = if let Ok(builder) = result {
            builder
        } else {
            result.unwrap_err()
        }
        .build();
        _ = simplelog::CombinedLogger::init(vec![simplelog::TermLogger::new(
            log::LevelFilter::Info,
            config,
            simplelog::TerminalMode::Mixed,
            simplelog::ColorChoice::Auto,
        )]);
    }
}

#[macro_export]
macro_rules! info {
    // info!(target: "my_target", key1 = 42, key2 = true; "a {} event", "log")
    // info!(target: "my_target", "a {} event", "log")
    (target: $target:expr, $($arg:tt)+) => {
        cfg_if::cfg_if! {
            if #[cfg(feature = "logs")] {
                $crate::log::init();
                log::info!(target: $target, $($arg)+)
            }
        }
    };

    // info!("a {} event", "log")
    ($($arg:tt)+) => {
        cfg_if::cfg_if! {
            if #[cfg(feature = "logs")] {
                $crate::log::init();
                log::info!($($arg)+)
            }
        }
    }
}

#[macro_export]
macro_rules! warn {
    // warn!(target: "my_target", key1 = 42, key2 = true; "a {} event", "log")
    // warn!(target: "my_target", "a {} event", "log")
    (target: $target:expr, $($arg:tt)+) => {
        cfg_if::cfg_if! {
            if #[cfg(feature = "logs")] {
                 $crate::log::init();
                log::warn!(target: $target, $($arg)+)
            }
        }
    };

    // warn!("a {} event", "log")
    ($($arg:tt)+) => {
        cfg_if::cfg_if! {
            if #[cfg(feature = "logs")] {
               $crate::log::init();
                log::warn!($($arg)+)
            }
        }
    }
}

#[macro_export]
macro_rules! error {
    // error!(target: "my_target", key1 = 42, key2 = true; "a {} event", "log")
    // error!(target: "my_target", "a {} event", "log")
    (target: $target:expr, $($arg:tt)+) => {
        cfg_if::cfg_if! {
            if #[cfg(feature = "logs")] {
                 $crate::log::init();
                log::error!(target: $target, $($arg)+)
            }
        }

    };

    // error!("a {} event", "log")
    ($($arg:tt)+) => {
        cfg_if::cfg_if! {
            if #[cfg(feature = "logs")] {
                 $crate::log::init();
                log::error!($($arg)+)
            }
        }

    }
}
