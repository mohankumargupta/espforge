//! `Board` as machine-readable JSON, for `espforge check --format json`.
//!
//! This lives in the CLI, not `espforge-model`: the model stays
//! dependency-free, and the field set is a presentation decision that belongs
//! next to the command that ships it. The shape is deliberately explicit
//! (no `serde` derive) so a new field is a visible edit here and a visible
//! snapshot change in the tests.
//!
//! Spans are byte offsets into the source, `[start, end)`, matching the
//! diagnostics format.

use espforge_model::board::{
    Board, Bus, BusId, Claim, Component, Ident, Parity, PinUse, Service, StopBits,
};
use espforge_model::{Console, Runtime};
use serde_json::{Value, json};

/// One board document.
pub fn value(board: &Board) -> Value {
    json!({
        "meta": {
            "name": board.meta.name,
            "description": board.meta.description,
            "chip": board.meta.chip,
            "runtime": runtime_name(board.meta.runtime),
            "console": console_name(board.meta.console),
        },
        "resources": {
            "pins": board.resources.pin_claims()
                .map(|(pin, claim)| json!({
                    "pin": pin.0,
                    "use": use_name(claim.need),
                    "owner": claim_value(claim),
                }))
                .collect::<Vec<_>>(),
            "peripherals": board.resources.peripheral_claims()
                .map(|(peripheral, claim)| json!({
                    "peripheral": peripheral,
                    "owner": claim_value(claim),
                }))
                .collect::<Vec<_>>(),
            "buses": board.resources.buses().iter().map(bus_value).collect::<Vec<_>>(),
        },
        "components": board.components.iter()
            .map(|component| component_value(board, component))
            .collect::<Vec<_>>(),
        "services": board.services.iter().map(service_value).collect::<Vec<_>>(),
    })
}

fn runtime_name(runtime: Runtime) -> &'static str {
    match runtime {
        Runtime::Blocking => "blocking",
        Runtime::Embassy => "embassy",
    }
}

fn console_name(console: Console) -> &'static str {
    match console {
        Console::Uart => "uart",
        Console::JtagSerial => "jtag-serial",
    }
}

fn use_name(use_: PinUse) -> &'static str {
    match use_ {
        PinUse::Input => "input",
        PinUse::Output => "output",
        PinUse::Bidirectional => "bidirectional",
        PinUse::Unspecified => "unspecified",
    }
}

fn claim_value(claim: &Claim) -> Value {
    json!({
        "kind": claim.owner.kind.tag(),
        "id": claim.owner.id.raw,
        "span": [claim.span.start, claim.span.end],
    })
}

fn ident_value(id: &Ident) -> Value {
    json!({
        "id": id.raw,
        "span": [id.span.start, id.span.end],
    })
}

fn bus_value(bus: &Bus) -> Value {
    match bus {
        Bus::I2c {
            id,
            peripheral,
            sda,
            scl,
            khz,
            used_by,
        } => json!({
            "kind": "i2c",
            "id": ident_value(id),
            "peripheral": peripheral,
            "sda": sda.pin().0,
            "scl": scl.pin().0,
            "khz": khz,
            "used_by": used_by.as_ref().map(|id| id.raw.clone()),
        }),
        Bus::Spi {
            id,
            peripheral,
            sclk,
            mosi,
            miso,
            khz,
            mode,
        } => json!({
            "kind": "spi",
            "id": ident_value(id),
            "peripheral": peripheral,
            "sclk": sclk.pin().0,
            "mosi": mosi.pin().0,
            "miso": miso.as_ref().map(|pin| pin.pin().0),
            "khz": khz,
            "mode": mode,
        }),
        Bus::Uart {
            id,
            peripheral,
            tx,
            rx,
            used_by,
        } => json!({
            "kind": "uart",
            "id": ident_value(id),
            "peripheral": peripheral,
            "tx": tx.pin().0,
            "rx": rx.pin().0,
            "used_by": used_by.as_ref().map(|id| id.raw.clone()),
        }),
    }
}

fn component_value(board: &Board, component: &Component) -> Value {
    match component {
        Component::Led(led) => json!({
            "kind": "led",
            "id": ident_value(&led.id),
            "pin": led.pin.pin().0,
            "active_low": led.active_low,
        }),
        Component::Button(button) => json!({
            "kind": "button",
            "id": ident_value(&button.id),
            "pin": button.pin.pin().0,
            "pull_up": button.pull_up,
        }),
        Component::I2c(i2c) => json!({
            "kind": "i2c_component",
            "id": ident_value(&i2c.id),
            "address": i2c.address,
            "bus": bus_name(board, i2c.bus),
        }),
        Component::Spi(spi) => json!({
            "kind": "spi_component",
            "id": ident_value(&spi.id),
            "bus": bus_name(board, spi.bus),
            "cs": spi.cs.pin().0,
            "mode": spi.mode,
        }),
        Component::Uart(uart) => json!({
            "kind": "uart_component",
            "id": ident_value(&uart.id),
            "bus": bus_name(board, uart.port),
            "baud": uart.baud,
            "parity": parity_name(uart.framing.parity),
            "stop_bits": stop_bits_value(uart.framing.stop_bits),
        }),
    }
}

fn parity_name(parity: Parity) -> &'static str {
    match parity {
        Parity::None => "none",
        Parity::Even => "even",
        Parity::Odd => "odd",
    }
}

fn stop_bits_value(stop_bits: StopBits) -> u8 {
    match stop_bits {
        StopBits::One => 1,
        StopBits::Two => 2,
    }
}

fn service_value(service: &Service) -> Value {
    match service {
        Service::Wifi(wifi) => json!({
            "kind": "wifi",
            "ssid": wifi.ssid,
            "password": wifi.password,
        }),
    }
}

/// The bus a component rides on, as the same id/span object buses use.
fn bus_name(board: &Board, id: BusId) -> Value {
    match board.resources.bus(id) {
        Some(bus) => ident_value(bus.id()),
        None => Value::Null,
    }
}
