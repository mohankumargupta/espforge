//! A tiny fixture driver: one identity register over I2C.
//!
//! It exists so the package path can be exercised without a real part: the
//! element is data in `testing/fixtures/drivers/accel.kdl`, and this crate is
//! the Rust half the manifest names.
//!
//! The endpoint is the same `RefCellDevice` the generated wiring builds for a
//! blocking I2C device, and the address is a constructor argument exactly as
//! `needs_address` says.

#![no_std]

use esp_hal::Blocking;
use esp_hal::i2c::master::I2c;
use espforge_runtime::DeviceErrorKind;
use espforge_runtime::embedded_hal::i2c::I2c as _;
use espforge_runtime::embedded_hal_bus::i2c::RefCellDevice;

/// The I2C endpoint the generated wiring builds.
pub type FxAccelEndpoint = RefCellDevice<'static, I2c<'static, Blocking>>;

/// The identity register: `0xA5` on the fixture.
const WHO_AM_I: u8 = 0x01;
/// What a healthy fixture answers with.
const WHO_AM_I_VALUE: u8 = 0xA5;

/// A fixture accelerometer on an I2C endpoint.
pub struct FxAccel {
    device: FxAccelEndpoint,
    address: u8,
}

impl FxAccel {
    /// Check the identity register and take the endpoint.
    pub fn new(device: FxAccelEndpoint, address: u8) -> Result<Self, DeviceErrorKind> {
        let mut this = Self { device, address };
        match this.who_am_i() {
            Ok(WHO_AM_I_VALUE) => Ok(this),
            _ => Err(DeviceErrorKind::Init),
        }
    }

    /// The identity register.
    pub fn who_am_i(&mut self) -> Result<u8, ()> {
        let mut id = [0u8; 1];
        self.device
            .write_read(self.address, &[WHO_AM_I], &mut id)
            .map_err(|_| ())?;
        Ok(id[0])
    }

    /// One sample: the fixture has no axes, so it reports zero.
    pub fn read(&mut self) -> Result<i16, ()> {
        Ok(0)
    }
}
