use std::ffi::c_char;
use std::ffi::CStr;

use crate::errors::{Error, Result};

macro_rules! log_cb {
    ($identifier: ident, $level: ident) => {
        extern "C" fn $identifier(msg: *const c_char) {
            unsafe {
                let c = CStr::from_ptr(msg);
                let decoded = c.to_string_lossy();
                log::$level!("{}", decoded);
            }
        }
    };
}

log_cb!(debug_cb, debug);
log_cb!(info_cb, info);
log_cb!(warning_cb, warn);
log_cb!(error_cb, error);

unsafe fn ensure_initialized_inner() -> Result<()> {
    if syzcall!(
        init_logging,
        Some(error_cb),
        Some(warning_cb),
        Some(info_cb),
        Some(debug_cb)
    ) == 0
    {
        return Err(Error::new("Unable to initialize logging"));
    }

    if syzcall!(init_context) == 0 {
        return Err(Error::new("Unable to initialize miniaudio context"));
    }

    Ok(())
}

pub(crate) fn ensure_initialized() -> Result<()> {
    lazy_static::lazy_static! {
        static ref INITIALIZED: Result<()> = unsafe { ensure_initialized_inner () };
    }

    match &*INITIALIZED {
        Ok(()) => Ok(()),
        Err(e) => Err(e.clone_internal()),
    }
}
