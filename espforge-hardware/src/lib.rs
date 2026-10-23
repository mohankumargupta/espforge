//! Hardware lowering: validated sections -> [`Hardware`].
//!
//! Step 1d (see `improve.md` item 1): per-bus lowering lives in
//! `gpio` / `i2c` / `spi` / `uart` / `wifi`, pin/peripheral clashes in
//! `conflicts`. This module is the thin orchestrator: check `# devices`,
//! dispatch the `# peripherals` / `# components` loops in spec order (so the
//! first error in file order wins, exactly like the old `pipeline::hardware`),
//! run [`conflicts::check_all`], then assemble [`Hardware`].
//!
//! `lower` takes the parsed sections map instead of the CLI's `Checked` so
//! this crate stays a pure library (`espforge` -> `espforge-hardware`, not
//! the reverse). `pipeline::hardware` is a one-line wrapper over it.
//!
//! ```text
//! mod gpio, i2c, spi, uart, wifi, conflicts, util
//! pub fn lower(sections) -> Result<Hardware, String>
//! pub fn buildable(sections) -> Result<(), String>
//! pub fn lower_sections_alias for tests
//! ```

pub mod conflicts;
pub mod gpio;
pub mod i2c;
pub mod spi;
pub mod uart;
pub mod util;
pub mod wifi;

// Re-export so lowering modules and callers share one import path.
pub use espforge_model::hardware::*;

use std::collections::HashMap;

use espf::context::ParsedNode;

/// The hardware and services `build` can generate code for today: `<gpio>`
/// peripherals used by `<led>` and `<button>` components, `<i2c>` peripherals
/// used by `<i2c_component>` components, `<spi>` peripherals used by
/// `<spi_component>` components, and one `<wifi>` service. Anything else is a
/// clear "not supported yet".
pub fn lower(sections: &HashMap<String, Vec<ParsedNode>>) -> Result<Hardware, String> {
    if sections
        .get("devices")
        .is_some_and(|v| !v.is_empty())
    {
        return Err(
            "`# devices` is not supported by `build` yet (only <gpio> peripherals with <led> and <button> components, <i2c> peripherals with <i2c_component> components, <spi> peripherals with <spi_component> components, and the <wifi> service)"
                .to_string(),
        );
    }

    let mut gpios: Vec<gpio::GpioEntry> = Vec::new();
    let mut i2c_buses: Vec<i2c::I2cBusDraft> = Vec::new();
    let mut spi_buses: Vec<spi::SpiBusDraft> = Vec::new();
    let mut uarts: Vec<uart::UartDraft> = Vec::new();

    for n in sections
        .get("peripherals")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if n.tag_name == "gpio" {
            gpios.push(gpio::lower_gpio(n)?);
        } else if n.tag_name == "i2c" {
            i2c_buses.push(i2c::lower_i2c(n)?);
        } else if n.tag_name == "spi" {
            spi_buses.push(spi::lower_spi(n)?);
        } else if n.tag_name == "uart" {
            uarts.push(uart::lower_uart(n)?);
        } else {
            return Err(format!(
                "<{}> in `# peripherals` is not supported by `build` yet (only <gpio>, <i2c>, <spi> and <uart>)",
                n.tag_name
            ));
        }
    }

    let mut spi_devices: Vec<spi::SpiDeviceDraft> = Vec::new();
    let mut leds: Vec<LedSpec> = Vec::new();
    let mut buttons: Vec<ButtonSpec> = Vec::new();

    for n in sections
        .get("components")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if n.tag_name == "i2c_component" {
            i2c::attach_i2c_component(n, &mut i2c_buses)?;
        } else if n.tag_name == "spi_component" {
            spi::attach_spi_component(n, &mut spi_buses, &mut spi_devices)?;
        } else if n.tag_name == "uart_component" {
            uart::attach_uart_component(n, &mut uarts)?;
        } else if n.tag_name == "button" {
            gpio::attach_button(n, &gpios, &leds, &mut buttons)?;
        } else if n.tag_name == "led" {
            gpio::attach_led(n, &gpios, &mut leds, &buttons)?;
        } else {
            return Err(format!(
                "<{}> in `# components` is not supported by `build` yet (only <led>, <button>, <i2c_component>, <spi_component> and <uart_component>)",
                n.tag_name
            ));
        }
    }

    let wifi = wifi::lower_services(
        sections
            .get("services")
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )?;

    conflicts::check_all(
        &gpios,
        &i2c_buses,
        &spi_buses,
        &spi_devices,
        &uarts,
        &leds,
        &buttons,
    )?;

    let i2cs = i2c::finish_i2c(i2c_buses);
    let (spi_bus_specs, spis) = spi::finish_spi(spi_buses, spi_devices);
    let uart_specs = uart::finish_uart(uarts);

    Ok(Hardware {
        leds,
        buttons,
        i2cs,
        spi_buses: spi_bus_specs,
        spis,
        uarts: uart_specs,
        wifi,
    })
}

/// Whether `build` can turn this spec into firmware.
pub fn buildable(sections: &HashMap<String, Vec<ParsedNode>>) -> Result<(), String> {
    lower(sections).map(|_| ())
}

/// Test helper: `buildable` over a sections map (mirrors the old
/// `pipeline::buildable(&Checked)` shape without the CLI type).
pub fn buildable_sections(sections: &HashMap<String, Vec<ParsedNode>>) -> Result<(), String> {
    buildable(sections)
}
