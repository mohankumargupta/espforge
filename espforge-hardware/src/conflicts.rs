//! Pin/peripheral ownership checks (step 1e, done as part of 1d).
//!
//! Item 2e: every clash returns a located [`EspfError`] pointing at the
//! first-named element's node, with the historical message text preserved in
//! `label` — several `check`/`build` tests assert on `"both use GPIO{n}"`
//! and `"tx and rx"`.

use std::collections::HashMap;

use super::peripherals::gpio::GpioEntry;
use super::peripherals::i2c::I2cBusDraft;
use super::peripherals::spi::{SpiBusDraft, SpiDeviceDraft, bus_pins};
use super::peripherals::uart::UartDraft;
use super::{ButtonSpec, LedSpec};
use espf::context::ParsedNode;
use espf::diag::EspfError;

/// Find a peripheral node by its `id`.
fn peripheral_node<'a>(
    sections: &'a HashMap<String, Vec<ParsedNode<'a>>>,
    id: &str,
) -> Option<&'a ParsedNode<'a>> {
    sections
        .get("peripherals")?
        .iter()
        .find(|n| n.id == id)
}

/// Find a component node by id or by Rust slug (`red-led` -> `red_led`).
fn component_node<'a>(
    sections: &'a HashMap<String, Vec<ParsedNode<'a>>>,
    slug_or_id: &str,
) -> Option<&'a ParsedNode<'a>> {
    sections.get("components")?.iter().find(|n| {
        n.id == slug_or_id || n.id.replace('-', "_") == slug_or_id
    })
}

/// A clash error pointing at the first-named element's node. `first_tag` is
/// one of `gpio|i2c|spi|uart` (peripherals, looked up by id) or
/// `led|button|spi_component` (components, looked up by id or slug).
fn conflict_err(
    sections: &HashMap<String, Vec<ParsedNode<'_>>>,
    first_tag: &str,
    first_key: &str,
    title: &str,
    label: String,
) -> Vec<EspfError> {
    let node = match first_tag {
        "gpio" | "i2c" | "spi" | "uart" => peripheral_node(sections, first_key),
        _ => component_node(sections, first_key),
    };
    match node {
        Some(n) => vec![EspfError::at_node(n, title, label)],
        None => vec![EspfError::at_span(0..0, title, label)],
    }
}

/// All pin/peripheral ownership checks. Called after every `lower_*` /
/// `attach_*` step, before assembling [`super::Hardware`].
#[allow(clippy::too_many_arguments)]
pub fn check_all(
    sections: &HashMap<String, Vec<ParsedNode<'_>>>,
    gpios: &[GpioEntry],
    i2c_buses: &[I2cBusDraft],
    spi_buses: &[SpiBusDraft],
    spi_devices: &[SpiDeviceDraft],
    uarts: &[UartDraft],
    leds: &[LedSpec],
    buttons: &[ButtonSpec],
) -> Result<(), Vec<EspfError>> {
    // A `<gpio>` that shares a pin with an I2C SDA/SCL would be a wiring bug;
    // refuse it the same way two `<led>`s on one pin are refused.
    for bus in i2c_buses {
        for gpio in gpios {
            if gpio.pin == bus.sda || gpio.pin == bus.scl {
                return Err(conflict_err(
                    sections,
                    "gpio",
                    &gpio.id,
                    "pin conflict",
                    format!(
                        "<gpio id=\"{}\"> and <i2c id=\"{}\"> both use GPIO{}",
                        gpio.id, bus.id, gpio.pin
                    ),
                ));
            }
        }
    }

    // The same for the three SPI bus pins, and between all buses so no pin is
    // ever owned by two things the generated code drives.
    // A led sharing a pin with an SPI bus pin is a wiring bug just the same.
    for bus in spi_buses {
        for pin in bus_pins(bus) {
            for gpio in gpios {
                if gpio.pin == pin {
                    return Err(conflict_err(
                        sections,
                        "gpio",
                        &gpio.id,
                        "pin conflict",
                        format!(
                            "<gpio id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                            gpio.id, bus.id
                        ),
                    ));
                }
            }
            for led in leds {
                if led.pin == pin {
                    return Err(conflict_err(
                        sections,
                        "led",
                        &led.name,
                        "pin conflict",
                        format!(
                            "<led id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                            led.name, bus.id
                        ),
                    ));
                }
            }
            for button in buttons {
                if button.pin == pin {
                    return Err(conflict_err(
                        sections,
                        "button",
                        &button.name,
                        "pin conflict",
                        format!(
                            "<button id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                            button.name, bus.id
                        ),
                    ));
                }
            }
            for i2c in i2c_buses {
                if pin == i2c.sda || pin == i2c.scl {
                    return Err(conflict_err(
                        sections,
                        "i2c",
                        &i2c.id,
                        "pin conflict",
                        format!(
                            "<i2c id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                            i2c.id, bus.id
                        ),
                    ));
                }
            }
            for other in spi_buses {
                if other.id != bus.id && bus_pins(other).contains(&pin) {
                    return Err(conflict_err(
                        sections,
                        "spi",
                        &bus.id,
                        "pin conflict",
                        format!(
                            "<spi id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                            bus.id, other.id
                        ),
                    ));
                }
            }
            // A device's chip-select may not be a bus pin of its own bus
            // either (that would hold CS low permanently).
            for dev in spi_devices {
                if dev.bus == bus.id && pin == dev.cs {
                    return Err(conflict_err(
                        sections,
                        "spi_component",
                        &dev.name,
                        "pin conflict",
                        format!(
                            "<spi_component id=\"{}\">'s cs and <spi id=\"{}\"> both use GPIO{pin}",
                            dev.name, bus.id
                        ),
                    ));
                }
            }
        }
    }

    // A chip-select may not clash with anything the wiring owns either: the
    // device would be selected (or a bus pin driven) as soon as the frame
    // begins.
    for dev in spi_devices {
        let pin = dev.cs;
        for gpio in gpios {
            // `cs="gpio9"` names the pin it takes; that `<gpio>` is the
            // chip-select itself, not a second thing driving the pin. A
            // component on it is still caught by the led/button loops below.
            if gpio.pin == pin && dev.cs_gpio.as_deref() != Some(gpio.id.as_str()) {
                return Err(conflict_err(
                    sections,
                    "gpio",
                    &gpio.id,
                    "pin conflict",
                    format!(
                        "<gpio id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                        gpio.id, dev.name
                    ),
                ));
            }
        }
        for led in leds {
            if led.pin == pin {
                return Err(conflict_err(
                    sections,
                    "led",
                    &led.name,
                    "pin conflict",
                    format!(
                        "<led id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                        led.name, dev.name
                    ),
                ));
            }
        }
        for button in buttons {
            if button.pin == pin {
                return Err(conflict_err(
                    sections,
                    "button",
                    &button.name,
                    "pin conflict",
                    format!(
                        "<button id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                        button.name, dev.name
                    ),
                ));
            }
        }
        for i2c in i2c_buses {
            if pin == i2c.sda || pin == i2c.scl {
                return Err(conflict_err(
                    sections,
                    "i2c",
                    &i2c.id,
                    "pin conflict",
                    format!(
                        "<i2c id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                        i2c.id, dev.name
                    ),
                ));
            }
        }
        for bus in spi_buses {
            if bus_pins(bus).contains(&pin) {
                return Err(conflict_err(
                    sections,
                    "spi",
                    &bus.id,
                    "pin conflict",
                    format!(
                        "<spi id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                        bus.id, dev.name
                    ),
                ));
            }
        }
    }

    // Two `<uart>`s claiming one physical peripheral would clash at init:
    // `Peripherals.UART1` (say) can only be consumed once.
    for i in 0..uarts.len() {
        for other in &uarts[..i] {
            if other.peripheral == uarts[i].peripheral {
                return Err(conflict_err(
                    sections,
                    "uart",
                    &other.id,
                    "peripheral clash",
                    format!(
                        "<uart id=\"{}\"> and <uart id=\"{}\"> both use {}",
                        other.id, uarts[i].id, uarts[i].peripheral
                    ),
                ));
            }
        }
    }

    // UART pins are owned wiring, exactly like the I2C and SPI pins above:
    // nothing else may drive or listen on a UART's TX/RX, and full-duplex TX
    // and RX may not even share a pin with each other.
    for uart in uarts {
        if uart.tx == uart.rx {
            return Err(conflict_err(
                sections,
                "uart",
                &uart.id,
                "uart wiring",
                format!(
                    "<uart id=\"{}\"> tx and rx cannot both be GPIO{}",
                    uart.id, uart.tx
                ),
            ));
        }
        for pin in [uart.tx, uart.rx] {
            for gpio in gpios {
                if gpio.pin == pin {
                    return Err(conflict_err(
                        sections,
                        "gpio",
                        &gpio.id,
                        "pin conflict",
                        format!(
                            "<gpio id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                            gpio.id, uart.id
                        ),
                    ));
                }
            }
            for led in leds {
                if led.pin == pin {
                    return Err(conflict_err(
                        sections,
                        "led",
                        &led.name,
                        "pin conflict",
                        format!(
                            "<led id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                            led.name, uart.id
                        ),
                    ));
                }
            }
            for button in buttons {
                if button.pin == pin {
                    return Err(conflict_err(
                        sections,
                        "button",
                        &button.name,
                        "pin conflict",
                        format!(
                            "<button id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                            button.name, uart.id
                        ),
                    ));
                }
            }
            for i2c in i2c_buses {
                if pin == i2c.sda || pin == i2c.scl {
                    return Err(conflict_err(
                        sections,
                        "i2c",
                        &i2c.id,
                        "pin conflict",
                        format!(
                            "<i2c id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                            i2c.id, uart.id
                        ),
                    ));
                }
            }
            for bus in spi_buses {
                if bus_pins(bus).contains(&pin) {
                    return Err(conflict_err(
                        sections,
                        "spi",
                        &bus.id,
                        "pin conflict",
                        format!(
                            "<spi id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                            bus.id, uart.id
                        ),
                    ));
                }
            }
            for dev in spi_devices {
                if dev.cs == pin {
                    return Err(conflict_err(
                        sections,
                        "spi_component",
                        &dev.name,
                        "pin conflict",
                        format!(
                            "<spi_component id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                            dev.name, uart.id
                        ),
                    ));
                }
            }
            for other in uarts {
                if other.id == uart.id {
                    continue;
                }
                if other.tx == pin || other.rx == pin {
                    return Err(conflict_err(
                        sections,
                        "uart",
                        &uart.id,
                        "pin conflict",
                        format!(
                            "<uart id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                            uart.id, other.id
                        ),
                    ));
                }
            }
        }
    }

    Ok(())
}
