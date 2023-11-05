// check https://www.rustwiki.org.cn/en/reference/introduction.html for help information
macro_rules! init_hook {
    ( $symbol:literal ) => {
        once_cell::sync::Lazy::new(|| unsafe {
            let syscall = $symbol;
            let symbol = std::ffi::CString::new(String::from(syscall))
                .unwrap_or_else(|_| panic!("can not transfer \"{syscall}\" to CString"));
            let ptr = libc::dlsym(libc::RTLD_NEXT, symbol.as_ptr());
            assert!(!ptr.is_null(), "system call \"{syscall}\" not found !");
            std::mem::transmute(ptr)
        })
    };
}

pub mod common;

pub mod sleep;

pub mod socket;

pub mod read;

pub mod write;

#[cfg(any(
    target_os = "linux",
    target_os = "l4re",
    target_os = "android",
    target_os = "emscripten"
))]
mod linux_like;
