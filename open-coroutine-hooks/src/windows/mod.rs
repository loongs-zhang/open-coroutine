use retour::static_detour;
use std::error::Error;
use std::ffi::{c_void, CString};
use windows_sys::Win32::Foundation::BOOL;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

#[no_mangle]
#[allow(non_snake_case, warnings)]
pub unsafe extern "system" fn DllMain(
    _module: *mut c_void,
    call_reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    if call_reason == DLL_PROCESS_ATTACH {
        // A console may be useful for printing to 'stdout'
        // winapi::um::consoleapi::AllocConsole();

        // Preferably a thread should be created here instead, since as few
        // operations as possible should be performed within `DllMain`.
        BOOL::from(main().is_ok())
    } else {
        1
    }
}

/// Called when the DLL is attached to the process.
unsafe fn main() -> Result<(), Box<dyn Error>> {
    let address =
        get_module_symbol_address("kernel32.dll", "Sleep").expect("could not find 'Sleep' address");
    let target: unsafe extern "system" fn(u32) = std::mem::transmute(address);
    static_detour! {
      static SLEEP: unsafe extern "system" fn(u32);
    }
    #[allow(non_snake_case)]
    fn Sleep(dw_milliseconds: u32) {
        open_coroutine_core::syscall::Sleep(Some(&SLEEP), dw_milliseconds);
    }
    SleepHook.initialize(target, Sleep)?.enable()?;
    Ok(())
}

/// Returns a module symbol's absolute address.
fn get_module_symbol_address(module: &str, symbol: &str) -> Option<usize> {
    let module = module
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<u16>>();
    let symbol = CString::new(symbol).unwrap();
    unsafe {
        let handle = GetModuleHandleW(module.as_ptr());
        GetProcAddress(handle, symbol.as_ptr().cast()).map(|n| n as usize)
    }
}
