//! The stable diagnostic codes this slice can reach, one per message.
//!
//! The ranges are reserved by stage (see the crate docs). A new message gets a
//! new entry here; the numbers are the legacy ones, never renumbered.
//!
//! Codes are data: the human renderer does not print them.

use crate::DiagCode;

macro_rules! codes {
    ($($(#[$doc:meta])* $name:ident = $code:literal;)*) => {
        $(
            $(#[$doc])*
            pub const $name: DiagCode = DiagCode::new($code);
        )*

        /// Every code defined in this module, in declaration order.
        pub const ALL: &[DiagCode] = &[$($name),*];
    };
}

codes! {
    /// A parser-junk line that is not a node.
    SYNTAX_NODE_LINE = "E0101";
    /// A line that should have been a `# section` header.
    SYNTAX_SECTION_HEADER = "E0102";
    /// Two `# section` blocks with the same name.
    STRUCTURE_DUPLICATE_SECTION = "E0103";
    /// A required `# section` block is absent.
    STRUCTURE_MISSING_SECTION = "E0104";

    /// An element the `# espforge` schema does not know.
    CORE_UNKNOWN_ELEMENT = "E0201";
    /// `# espforge` lacks `<project>`, `<chip>` or `<runtime>`.
    CORE_INCOMPLETE = "E0202";
    /// The same element is written twice in `# espforge`.
    CORE_DUPLICATE_ELEMENT = "E0203";
    /// `<chip>` without a `type` attribute.
    CORE_CHIP_WITHOUT_TYPE = "E0204";
    /// A chip name the catalog does not know.
    CORE_UNKNOWN_CHIP = "E0205";
    /// `<runtime>` without a `type` attribute.
    CORE_RUNTIME_WITHOUT_TYPE = "E0206";
    /// A runtime name the catalog does not know.
    CORE_UNKNOWN_RUNTIME = "E0207";
    /// `<console>` without a `type` attribute.
    CORE_CONSOLE_WITHOUT_TYPE = "E0208";
    /// A console name the catalog does not know.
    CORE_UNKNOWN_CONSOLE = "E0209";

    /// An option name that is not in the schema.
    OPTION_UNKNOWN = "E0301";
    /// An option value outside its declared choices.
    OPTION_INVALID_VALUE = "E0302";
    /// An option value that cannot be set in `<options>`.
    OPTION_DERIVED = "E0306";
    /// An option that needs another option that is off.
    OPTION_REQUIRES = "E0307";
    /// An option that the selected chip cannot provide.
    OPTION_UNAVAILABLE = "E0308";
    /// Two options in one selection group are on together.
    OPTION_MUTUALLY_EXCLUSIVE = "E0309";
    /// The selected console does not exist on the chip.
    OPTION_NO_CONSOLE = "E0310";
    /// An attribute value with the wrong type.
    VALUE_WRONG_TYPE = "E0311";
    /// A value that should be a boolean.
    VALUE_NOT_BOOLEAN = "E0312";
    /// A value that should be an unsigned number.
    VALUE_NOT_NUMBER = "E0313";
    /// A key used both as a plain value and as an object.
    VALUE_KEY_KIND = "E0314";
    /// A required attribute is absent.
    VALUE_MISSING_ATTRIBUTE = "E0315";

    /// A peripheral tag no element implements.
    PERIPHERAL_UNSUPPORTED = "E0501";
    /// `<gpio pin>` that is not a number.
    GPIO_INVALID_PIN = "E0502";
    /// `<gpio direction>` outside `input`/`output`.
    GPIO_INVALID_DIRECTION = "E0503";
    /// An `<i2c>` id that is not `i2c0`, `i2c1` or `i2c2`.
    I2C_INVALID_ID = "E0504";
    /// An `<i2c>` pin that is not a GPIO number.
    I2C_INVALID_PIN = "E0505";
    /// An `<i2c frequency>` that is not `100kHz`-shaped.
    I2C_INVALID_FREQUENCY = "E0506";
    /// An `<spi>` id other than `spi2`.
    SPI_INVALID_ID = "E0507";
    /// An `<spi>` pin that is not a GPIO number.
    SPI_INVALID_PIN = "E0508";
    /// An `<spi mode>` outside 0..=3.
    SPI_INVALID_MODE = "E0509";
    /// An `<spi frequency>` that is not `100kHz`-shaped.
    SPI_INVALID_FREQUENCY = "E0510";
    /// A `<uart>` id that is not `uart0`..`uart3`.
    UART_INVALID_ID = "E0511";
    /// A `<uart uart=>` value outside `UART0`..`UART3`.
    UART_INVALID_PERIPHERAL = "E0512";
    /// A `<uart>` pin that is not a GPIO number.
    UART_INVALID_PIN = "E0513";
    /// A `<wifi>` on a chip with no Wi-Fi radio.
    WIFI_NO_RADIO = "E0514";
    /// A `<wifi>` whose id is missing or malformed.
    WIFI_INVALID_ID = "E0515";

    /// An `<led>` that cannot be built.
    COMPONENT_INVALID_LED = "E0601";
    /// A `<button>` that cannot be built.
    COMPONENT_INVALID_BUTTON = "E0602";
    /// An `<i2c_component>` that cannot be built.
    COMPONENT_INVALID_I2C = "E0603";
    /// An `<spi_component>` that cannot be built.
    COMPONENT_INVALID_SPI = "E0604";
    /// A `<uart_component>` that cannot be built.
    COMPONENT_INVALID_UART = "E0605";
    /// A component tag no element implements.
    COMPONENT_UNSUPPORTED = "E0606";
    /// An `<spi_component cs>` naming a gpio that does not exist.
    COMPONENT_INVALID_CS = "E0607";
    /// An `<led>` referencing a gpio id that does not exist.
    COMPONENT_UNKNOWN_GPIO = "E0608";
    /// A led on an input pin.
    COMPONENT_WRONG_DIRECTION = "E0609";
    /// A `<uart_component baud_rate="0">`.
    COMPONENT_INVALID_BAUD = "E0610";
    /// A second `<uart_component>` on one `<uart>` port.
    COMPONENT_UART_USED = "E0612";
    /// A bus component referring to a bus that does not exist.
    COMPONENT_UNKNOWN_BUS = "E0613";
    /// A bus component id that cannot become a Rust identifier.
    COMPONENT_INVALID_ID = "E0614";
    /// A recipe component that cannot be built (the recipe's default code).
    /// Legacy component codes stop at `E0614`; this one is free.
    COMPONENT_INVALID = "E0615";
    /// A `<wifi_station wifi>` reference to a radio that does not exist.
    COMPONENT_UNKNOWN_WIFI = "E0616";
    /// A second `<wifi_station>` on one `<wifi>` radio.
    COMPONENT_WIFI_RADIO_USED = "E0617";
    /// An `ssid` that is empty or longer than 32 bytes.
    COMPONENT_INVALID_SSID = "E0618";
    /// A password that is required but missing, or outside 8..=63 characters.
    COMPONENT_INVALID_PASSWORD = "E0619";
    /// `auth="open"` written together with a password.
    COMPONENT_OPEN_WITH_PASSWORD = "E0620";
    /// A `password="env:NAME"` whose variable name is not an identifier.
    COMPONENT_INVALID_ENV = "E0621";
    /// A `<wifi_station>` that cannot be built (missing or invalid attribute).
    COMPONENT_INVALID_WIFI = "E0622";

    /// Two elements of different kinds on one pin.
    CONFLICT_PIN = "E0701";
    /// A pin claimed by two elements of the same kind.
    CONFLICT_PIN_USED = "E0702";
    /// One element needs an input and the other an output.
    CONFLICT_DIRECTION = "E0703";
    /// Two elements naming the same hardware peripheral (`UART1`).
    CONFLICT_PERIPHERAL = "E0704";
    /// An `<i2c>` that puts sda and scl on one pin.
    CONFLICT_I2C_WIRING = "E0705";
    /// An `<spi>` that puts two of its pins on one GPIO.
    CONFLICT_SPI_WIRING = "E0706";
    /// A `<uart>` that puts tx and rx on one pin.
    CONFLICT_UART_WIRING = "E0707";
    /// Two addressed `<i2c_component>`s on one `<i2c>` bus sharing an address.
    CONFLICT_I2C_ADDRESS = "E0708";

    /// A service `network=` reference to an interface that does not exist.
    SERVICE_UNKNOWN_NETWORK = "E0801";
    /// A network service declared under the blocking runtime.
    SERVICE_REQUIRES_EMBASSY = "E0802";
    /// A service buffer below the minimum its protocol needs.
    SERVICE_BUFFER_TOO_SMALL = "E0803";
    /// `<https_client insecure="true">`: certificates will not be validated.
    SERVICE_INSECURE_WARNING = "W0804";
    /// A service that cannot be built: a bad id or a missing attribute.
    SERVICE_INVALID = "E0805";
    /// A service tag no element implements.
    SERVICE_UNSUPPORTED = "E0806";

    /// A device's bus reference to a component that does not exist.
    DEVICE_UNKNOWN_COMPONENT = "E0901";
    /// A device's auxiliary pin reference to a `<gpio>` that does not exist.
    DEVICE_UNKNOWN_GPIO = "E0902";
    /// A device's auxiliary pin on a GPIO declared as an input.
    DEVICE_WRONG_DIRECTION = "E0903";
    /// A device id that cannot become a Rust identifier.
    DEVICE_INVALID_ID = "E0904";
    /// A device tag no element implements.
    DEVICE_UNSUPPORTED = "E0905";
    /// A blocking-only display declared under the embassy runtime.
    DEVICE_REQUIRES_BLOCKING = "E0906";
    /// A device that cannot be built: a missing or invalid attribute.
    DEVICE_INVALID = "E0907";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn well_formed(code: &str) -> bool {
        let bytes = code.as_bytes();
        bytes.len() == 5
            && matches!(bytes[0], b'E' | b'W')
            && bytes[1..].iter().all(u8::is_ascii_digit)
    }

    #[test]
    fn every_code_is_well_formed() {
        for code in ALL {
            assert!(
                well_formed(code.as_str()),
                "{} is not `E`/`W` + four digits",
                code.as_str()
            );
        }
    }

    #[test]
    fn no_two_codes_share_a_number() {
        let mut seen = std::collections::BTreeSet::new();
        for code in ALL {
            assert!(
                seen.insert(code.as_str()),
                "{} is declared twice",
                code.as_str()
            );
        }
    }

    #[test]
    fn the_slice_only_defines_the_codes_it_can_reach() {
        for code in ALL {
            let number = &code.as_str()[1..];
            let n: u32 = number.parse().unwrap();
            assert!(
                matches!(
                    n,
                    101..=104
                        | 201..=209
                        | 301
                        | 302
                        | 306..=315
                        | 501..=515
                        | 601..=622
                        | 701..=708
                        | 801..=806
                        | 901..=907
                ),
                "{} is outside the codes this slice defines",
                code.as_str()
            );
        }
    }
}
