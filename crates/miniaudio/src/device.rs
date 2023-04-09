use std::borrow::Cow;
use std::ffi::CStr;
use std::num::NonZeroU32;

use crate::c_binding as c;
use crate::errors::{Error, Result};
use crate::miniaudio_dispatch::{dispatch, dispatch_exclusive};

pub struct DeviceInfo {
    underlying: c::device_info,
}

/// Formats a device may be asked for.
#[derive(Debug)]
pub enum DeviceChannelFormat {
    /// One channel.
    Mono,

    /// 2 channels, [l, r].
    Stereo,

    /// Ask the device to do whatever it wants rather than trying to mix anything.
    Raw(NonZeroU32),
}

/// Options for opening a device.
///
/// Unless otherwise documented `Option<>` fields are interpreted such that `None` means whatever the device prefers.
#[derive(Debug, Default)]
pub struct DeviceOptions {
    pub channel_format: Option<DeviceChannelFormat>,

    /// If unset, use the device default.  If set, use Miniaudio's resampler, which is of low quality.
    pub sample_rate: Option<NonZeroU32>,
}

/// Information on what a device actually has.
pub struct DeviceConfig {
    underlying: c::device_config,
}

/// A running device.
pub struct DeviceHandle {
    handle: *mut std::ffi::c_void,
    userdata: *mut std::ffi::c_void,
    userdata_free: fn(*mut std::ffi::c_void),
}

/// Internal type that we put behind userdata in order to hold the user's callback.
struct DeviceUserdata<T> {
    callback: T,
}

unsafe impl Send for DeviceInfo {}
unsafe impl Sync for DeviceInfo {}
unsafe impl Send for DeviceHandle {}
unsafe impl Sync for DeviceHandle {}

impl Drop for DeviceInfo {
    fn drop(&mut self) {
        unsafe {
            syzcall!(
                device_info_deinit,
                &mut self.underlying as *mut c::device_info
            )
        }
    }
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        unsafe {
            syzcall!(playback_device_destroy, self.handle);
            (self.userdata_free)(self.userdata);
        }
    }
}

pub fn enumerate_output_devices() -> Result<impl Iterator<Item = DeviceInfo>> {
    unsafe extern "C" fn dev_cb(dev: *const c::device_info, ud: *mut std::ffi::c_void) {
        let v = ud as *mut Vec<DeviceInfo>;
        unsafe {
            v.as_mut().unwrap().push(DeviceInfo {
                underlying: dev.read(),
            })
        }
    }

    let results = dispatch(move || -> Result<Vec<DeviceInfo>> {
        let mut results: Vec<DeviceInfo> = vec![];
        unsafe {
            if syzcall!(
                enumerate_output_devices,
                Some(dev_cb),
                &mut results as *mut Vec<_> as *mut std::ffi::c_void
            ) == 0
            {
                return Err(Error::new(
                    "Unable to call Miniaudio to get the device list",
                ));
            }
        }
        Ok(results)
    })??;

    Ok(results.into_iter())
}

extern "C" fn device_callback<T: FnMut(&DeviceConfig, &mut [f32]) + 'static>(
    output: *mut f32,
    output_size: u64,
    config: *const c::device_config,
    ud: *mut std::ffi::c_void,
) {
    unsafe {
        let ud = (ud as *mut DeviceUserdata<T>).as_mut().unwrap();
        let dest_slice = std::slice::from_raw_parts_mut(output, output_size as usize);
        let cfg = DeviceConfig {
            underlying: config.read(),
        };
        (ud.callback)(&cfg, dest_slice);
    }
}

impl DeviceInfo {
    /// Get a human-readable name for this device.
    pub fn name(&self) -> Cow<str> {
        let c = unsafe { CStr::from_ptr(self.underlying.name) };
        c.to_string_lossy()
    }

    pub fn is_platform_default(&self) -> bool {
        self.underlying.is_platform_default != 0
    }
}

impl DeviceConfig {
    pub fn channels(&self) -> u32 {
        self.underlying.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.underlying.sr
    }
}

impl<T: Send + FnMut(&DeviceConfig, &mut [f32]) + 'static> DeviceUserdata<T> {
    fn new(callback: T) -> Self {
        DeviceUserdata { callback }
    }

    fn into_void_and_callback(self) -> (*mut std::ffi::c_void, fn(*mut std::ffi::c_void)) {
        let boxed = Box::new(self);
        let vptr = Box::into_raw(boxed) as *mut std::ffi::c_void;

        fn freer<T>(x: *mut std::ffi::c_void) {
            unsafe {
                let got: Box<T> = Box::from_raw(x as *mut T);
                std::mem::drop(got);
            }
        }

        (vptr, freer::<T>)
    }
}

impl DeviceChannelFormat {
    pub fn channel_count(&self) -> NonZeroU32 {
        match self {
            Self::Mono => NonZeroU32::new(1).unwrap(),
            Self::Stereo => NonZeroU32::new(2).unwrap(),
            Self::Raw(x) => *x,
        }
    }
}

fn open_device_inner<CB: FnMut(&DeviceConfig, &mut [f32]) + Send + 'static>(
    device: Option<&DeviceInfo>,
    opts: &DeviceOptions,
    callback: CB,
) -> Result<DeviceHandle> {
    let c_opts = c::device_options {
        channels: opts
            .channel_format
            .as_ref()
            .map(|x| x.channel_count().get())
            .unwrap_or(0),
        sr: opts.sample_rate.as_ref().map(|x| x.get()).unwrap_or(0),
        device_id: device
            .map(|x| x.underlying.id)
            .unwrap_or(std::ptr::null_mut()),
    };

    let (userdata, userdata_free) = DeviceUserdata::new(callback).into_void_and_callback();
    let dev = dispatch_exclusive(move || unsafe {
        syzcall!(
            playback_device_open,
            &c_opts as *const c::device_options,
            Some(device_callback::<CB>),
            userdata
        )
    })?;

    if dev.is_null() {
        return Err(Error::new("Unable to open Miniaudio device"));
    }

    Ok(DeviceHandle {
        handle: dev,
        userdata,
        userdata_free,
    })
}

/// Open a specified device.
pub fn open_playback_device(
    info: &DeviceInfo,
    opts: &DeviceOptions,
    callback: impl FnMut(&DeviceConfig, &mut [f32]) + Send + 'static,
) -> Result<DeviceHandle> {
    open_device_inner(Some(info), opts, callback)
}

/// Open the system's default device.
pub fn open_default_playback_device(
    opts: &DeviceOptions,
    callback: impl FnMut(&DeviceConfig, &mut [f32]) + Send + 'static,
) -> Result<DeviceHandle> {
    open_device_inner(None, opts, callback)
}

impl DeviceHandle {
    pub fn start(&mut self) -> Result<()> {
        let h = self.handle;
        if dispatch(move || unsafe { syzcall!(playback_device_start, h) })? == 0 {
            return Err(Error::new("Unable to start device"));
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        let h = self.handle;
        if dispatch(move || unsafe { syzcall!(playback_device_stop, h) })? == 0 {
            return Err(Error::new("Unable to stop device"));
        }
        Ok(())
    }
}
