pub use Sleep::Sleep;

#[allow(unused_macros)]
macro_rules! impl_raw {
    ( $struct_name: ident, $trait_name: ident, $($mod_name: ident)::*, $syscall: ident($($arg: ident : $arg_type: ty),*) -> $result: ty ) => {
        #[derive(Debug, Copy, Clone, Default)]
        struct $struct_name {}

        impl $trait_name for $struct_name {
            extern "system" fn $syscall(
                &self,
                fn_ptr: Option<&retour::StaticDetour<unsafe extern "system" fn($($arg_type),*)> -> $result>,
                $($arg: $arg_type),*
            ) -> $result {
                unsafe {
                    if let Some(f) = fn_ptr {
                        f.call($($arg),*)
                    } else {
                        $($mod_name)::*::$syscall($($arg),*)
                    }
                }
            }
        }
    }
}

mod Sleep;
