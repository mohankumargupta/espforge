//! `spi` component: an SPI master bus (ADR-003/008 bus-sharing).
//!
//! Mirrors v1's `espforge_components::components::spi::SPI`: a `Copy` handle
//! around a `&'static RefCell<Spi>`. The actual peripheral is allocated once in
//! a `StaticCell` by the generated wiring (the `spi` driver's `construct`), so
//! this type is cheap to move into devices — copying the handle is a pointer
//! bitcopy, never a move of the peripheral (no double-move, ADR-008).
//!
//! A bus-level chip-select is attached only when the `esp32.spi` declaration
//! provides one (some buses, e.g. the `ili9341` example, share a bus across
//! devices that each own a private CS pin instead — see `SpiDevice` below).

use core::cell::RefCell;
use esp_hal::gpio::{InputPin, Output, OutputPin};
use esp_hal::spi::master::{Config, Spi};
use esp_hal::spi::Mode;

use crate::Delay;

/// A `Copy` handle to a shared SPI master bus (v1-style, ADR-003).
#[derive(Clone, Copy)]
pub struct SpiBus {
    bus: &'static RefCell<Spi<'static, esp_hal::Blocking>>,
}

impl SpiBus {
    /// Wrap a `&'static RefCell<Spi>` allocated by the wiring code. The `spi`
    /// component driver builds the inner `Spi` once and hands out `Copy`
    /// handles to every device that shares this bus.
    pub fn from_ref(bus: &'static RefCell<Spi<'static, esp_hal::Blocking>>) -> Self {
        SpiBus { bus }
    }

    /// Build the owned esp-hal `Spi` master from its peripheral + pins and
    /// mode/frequency. `cs`, if present, is attached to the master so transfers
    /// manage it automatically; pass `None` when devices sharing the bus
    /// provide their own CS (wrap them with `SpiDevice`). Called once by the
    /// generated wiring; the result is parked in a `StaticCell<RefCell<_>>`
    /// and surfaced via `from_ref`.
    #[allow(clippy::too_many_arguments)]
    pub fn build<CS: OutputPin + 'static>(
        spi: esp_hal::peripherals::SPI2<'static>,
        mosi: impl OutputPin + 'static,
        miso: impl InputPin + 'static,
        sclk: impl OutputPin + 'static,
        cs: Option<CS>,
        mode: u8,
        frequency_khz: u32,
    ) -> Spi<'static, esp_hal::Blocking> {
        let spi_mode = match mode {
            0 => Mode::_0,
            1 => Mode::_1,
            2 => Mode::_2,
            3 => Mode::_3,
            _ => Mode::_0,
        };
        let config = Config::default()
            .with_frequency(esp_hal::time::Rate::from_khz(frequency_khz))
            .with_mode(spi_mode);
        let mut bus = Spi::new(spi, config)
            .unwrap()
            .with_sck(sclk)
            .with_mosi(mosi)
            .with_miso(miso);
        if let Some(cs) = cs {
            bus = bus.with_cs(cs);
        }
        bus
    }

    /// Shared access to the underlying bus.
    pub fn bus(&self) -> &'static RefCell<Spi<'static, esp_hal::Blocking>> {
        self.bus
    }
}

/// A single device on a shared SPI bus with its own chip-select pin — the
/// move-by-value analog of v1's `espforge_platform::bus::SpiDevice`. Used by
/// devices (like `ili9341`) that need `embedded_hal::spi::SpiDevice` (CS
/// managed per-transaction) rather than the bus-level CS `SpiBus` offers.
pub struct SpiDevice {
    bus: SpiBus,
    cs: Output<'static>,
    delay: Delay,
}

impl SpiDevice {
    pub fn new(bus: SpiBus, cs: Output<'static>, delay: Delay) -> Self {
        SpiDevice { bus, cs, delay }
    }

    /// `Delay` is `Copy`, so the device can take its own clone for the driver's
    /// `&mut impl DelayNs` init argument without moving it out of the handle.
    pub fn delay_clone(&self) -> Delay {
        self.delay
    }
}

impl embedded_hal::spi::ErrorType for SpiDevice {
    type Error = esp_hal::spi::Error;
}

impl embedded_hal::spi::SpiDevice for SpiDevice {
    fn transaction(
        &mut self,
        operations: &mut [embedded_hal::spi::Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        use embedded_hal::delay::DelayNs;
        use embedded_hal::spi::Operation;
        self.cs.set_low();
        let result = (|| {
            // Borrow the shared peripheral once; `&mut Spi` implements
            // `embedded_hal::spi::SpiBus` (via `impl SpiBus for &mut T`).
            let mut bus = self.bus.bus().borrow_mut();
            let spi = &mut *bus;
            for op in operations.iter_mut() {
                match op {
                    Operation::Read(buf) => embedded_hal::spi::SpiBus::read(spi, buf)?,
                    Operation::Write(buf) => embedded_hal::spi::SpiBus::write(spi, buf)?,
                    Operation::Transfer(read, write) => {
                        embedded_hal::spi::SpiBus::transfer(spi, read, write)?
                    }
                    Operation::TransferInPlace(buf) => {
                        embedded_hal::spi::SpiBus::transfer_in_place(spi, buf)?
                    }
                    Operation::DelayNs(ns) => {
                        self.delay.delay_ns((*ns).try_into().unwrap_or(u32::MAX))
                    }
                }
            }
            Ok(())
        })();
        self.cs.set_high();
        result
    }
}
