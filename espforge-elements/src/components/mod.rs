//! Elements that live in `# components`: a peripheral put to a purpose.

pub mod i2c;
pub mod recipe;
pub mod spi;
pub mod uart;
pub mod wifi_station;

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

use crate::model::{Parity, Runtime, StopBits};

/// The esp-hal driver-mode type for the project's runtime.
pub(crate) fn runtime_type(runtime: Runtime) -> TokenStream {
    match runtime {
        Runtime::Blocking => quote! { esp_hal::Blocking },
        Runtime::Embassy => quote! { esp_hal::Async },
    }
}

/// `unsafe { registry.peripherals.GPIO<pin>.clone_unchecked() }`.
pub(crate) fn gpio(pin: u8) -> TokenStream {
    let gpio = format_ident!("GPIO{}", pin);
    quote! { unsafe { registry.peripherals.#gpio.clone_unchecked() } }
}

/// `unsafe { registry.peripherals.<name>.clone_unchecked() }`.
pub(crate) fn peripheral_access(name: &str) -> TokenStream {
    let peripheral = format_ident!("{}", name);
    quote! { unsafe { registry.peripherals.#peripheral.clone_unchecked() } }
}

/// An integer literal spelled as the templates spell it: no type suffix.
pub(crate) fn int(value: u32) -> Literal {
    Literal::u32_unsuffixed(value)
}

/// `esp_hal::time::Rate::from_khz(<khz>)`.
pub(crate) fn rate(khz: u32) -> TokenStream {
    let khz = int(khz);
    quote! { esp_hal::time::Rate::from_khz(#khz) }
}

pub(crate) fn parity(value: Parity) -> TokenStream {
    match value {
        Parity::None => quote! { esp_hal::uart::Parity::None },
        Parity::Even => quote! { esp_hal::uart::Parity::Even },
        Parity::Odd => quote! { esp_hal::uart::Parity::Odd },
    }
}

pub(crate) fn stop_bits(value: StopBits) -> TokenStream {
    match value {
        StopBits::One => quote! { esp_hal::uart::StopBits::_1 },
        StopBits::Two => quote! { esp_hal::uart::StopBits::_2 },
    }
}

pub(crate) fn spi_mode(value: u8) -> TokenStream {
    let mode = format_ident!("_{}", value);
    quote! { esp_hal::spi::Mode::#mode }
}
