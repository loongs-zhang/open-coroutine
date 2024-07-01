pub use nanosleep::nanosleep;
pub use sleep::sleep;
pub use usleep::usleep;

#[allow(unused_macros)]
macro_rules! impl_raw {
    ( $struct_name: ident, $trait_name: ident, $syscall: ident($($arg: ident : $arg_type: ty),*) -> $result: ty ) => {
        #[derive(Debug, Copy, Clone, Default)]
        struct $struct_name {}

        impl $trait_name for $struct_name {
            extern "C" fn $syscall(
                &self,
                fn_ptr: Option<&extern "C" fn($($arg_type),*) -> $result>,
                $($arg: $arg_type),*
            ) -> $result {
                if let Some(f) = fn_ptr {
                    (f)($($arg),*)
                } else {
                    unsafe { libc::$syscall($($arg),*) }
                }
            }
        }
    }
}

mod nanosleep;
mod sleep;
mod usleep;
