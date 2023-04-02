use cpal::{
    traits::{DeviceTrait, HostTrait},
    Device,
};

use crate::error::{Error, Result};

/// An identifier for an audio device.
///
/// The [std::fmt::Display] impl produces a human-readable device name for UI purposes.
///
/// Identifiers are currently not guaranteed to be stable between app runs.  Currently, the underlying implementation is
/// [cpal], which doesn't offer this functionality.
pub struct DeviceIdentifier {
    device: Device,
    name: String,
}

impl std::fmt::Display for DeviceIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::write;
        write!(f, "{}", self.name)?;
        Ok(())
    }
}

/// Get all output devices available on this platform.
pub fn get_all_output_devices() -> Result<impl Iterator<Item = DeviceIdentifier>> {
    let host = cpal::default_host();
    let devices = host.output_devices().map_err(|e| Error::AudioBackend {
        message: e.to_string(),
    })?;

    devices
        .map(|device| {
            let name = device.name().map_err(|e| Error::AudioBackend {
                message: e.to_string(),
            })?;
            Ok(DeviceIdentifier { device, name })
        })
        .collect::<Result<Vec<_>>>()
        .map(|x| x.into_iter())
}

/// Get the default output device for this platform.
pub fn get_default_output_device() -> Result<DeviceIdentifier> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| Error::AudioBackend {
            message: "No default device is available".to_string(),
        })?;
    let name = device.name().map_err(|e| Error::AudioBackend {
        message: e.to_string(),
    })?;
    Ok(DeviceIdentifier { device, name })
}
