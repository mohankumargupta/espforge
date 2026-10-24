//! The stable diagnostic codes, one per message.
//!
//! The ranges are reserved by stage (see the crate docs). Keeping the map in
//! one place is what stops two stages from picking the same number: a new
//! message gets a new entry here, and [`for_message`] is the only producer of
//! a code for the `EspfError` shim.
//!
//! Codes are data: the human renderer does not print them, `to_json` does.
//! Once a code has shipped it is a public contract, so never renumber one for
//! a different message.

use crate::DiagCode;

/// A parser-junk line that is not a node.
pub const SYNTAX_NODE_LINE: DiagCode = DiagCode::new("E0101");
/// A line that should have been a `# section` header.
pub const SYNTAX_SECTION_HEADER: DiagCode = DiagCode::new("E0102");
/// Two `# section` blocks with the same name.
pub const STRUCTURE_DUPLICATE_SECTION: DiagCode = DiagCode::new("E0103");
/// A required `# section` block is absent.
pub const STRUCTURE_MISSING_SECTION: DiagCode = DiagCode::new("E0104");

/// An element the `# espforge` schema does not know.
pub const CORE_UNKNOWN_ELEMENT: DiagCode = DiagCode::new("E0201");
/// `# espforge` lacks `<project>`, `<chip>` or `<runtime>`.
pub const CORE_INCOMPLETE: DiagCode = DiagCode::new("E0202");
/// The same element is written twice in `# espforge`.
pub const CORE_DUPLICATE_ELEMENT: DiagCode = DiagCode::new("E0203");
/// `<chip>` without a `type` attribute.
pub const CORE_CHIP_WITHOUT_TYPE: DiagCode = DiagCode::new("E0204");
/// A chip name the catalog does not know.
pub const CORE_UNKNOWN_CHIP: DiagCode = DiagCode::new("E0205");
/// `<runtime>` without a `type` attribute.
pub const CORE_RUNTIME_WITHOUT_TYPE: DiagCode = DiagCode::new("E0206");
/// A runtime name the catalog does not know.
pub const CORE_UNKNOWN_RUNTIME: DiagCode = DiagCode::new("E0207");
/// `<console>` without a `type` attribute.
pub const CORE_CONSOLE_WITHOUT_TYPE: DiagCode = DiagCode::new("E0208");
/// A console name the catalog does not know.
pub const CORE_UNKNOWN_CONSOLE: DiagCode = DiagCode::new("E0209");

/// An option name that is not in the schema.
pub const OPTION_UNKNOWN: DiagCode = DiagCode::new("E0301");
/// An option value outside its declared choices.
pub const OPTION_INVALID_VALUE: DiagCode = DiagCode::new("E0302");
/// An option value that cannot be set in `<options>`.
pub const OPTION_DERIVED: DiagCode = DiagCode::new("E0306");
/// An option that needs another option that is off.
pub const OPTION_REQUIRES: DiagCode = DiagCode::new("E0307");
/// An option that the selected chip cannot provide.
pub const OPTION_UNAVAILABLE: DiagCode = DiagCode::new("E0308");
/// Two options in one selection group are on together.
pub const OPTION_MUTUALLY_EXCLUSIVE: DiagCode = DiagCode::new("E0309");
/// The selected console does not exist on the chip.
pub const OPTION_NO_CONSOLE: DiagCode = DiagCode::new("E0310");
/// An attribute value with the wrong type.
pub const VALUE_WRONG_TYPE: DiagCode = DiagCode::new("E0311");
/// A value that should be a boolean.
pub const VALUE_NOT_BOOLEAN: DiagCode = DiagCode::new("E0312");
/// A value that should be an unsigned number.
pub const VALUE_NOT_NUMBER: DiagCode = DiagCode::new("E0313");
/// A key used both as a plain value and as an object.
pub const VALUE_KEY_KIND: DiagCode = DiagCode::new("E0314");
/// A required attribute is absent.
pub const VALUE_MISSING_ATTRIBUTE: DiagCode = DiagCode::new("E0315");
/// A command-line answer that cannot be applied (no source span).
pub const CLI_ANSWER: DiagCode = DiagCode::new("E0399");

/// A `$` reference to a peripheral id that does not exist.
pub const GRAPH_UNKNOWN_PERIPHERAL: DiagCode = DiagCode::new("E0401");
/// A `$` reference to a component id that does not exist.
pub const GRAPH_UNKNOWN_COMPONENT: DiagCode = DiagCode::new("E0402");
/// A peripheral without an id.
pub const GRAPH_PERIPHERAL_WITHOUT_ID: DiagCode = DiagCode::new("E0403");
/// A component without an id.
pub const GRAPH_COMPONENT_WITHOUT_ID: DiagCode = DiagCode::new("E0404");
/// A component without its `i2c=`/`spi=`/`gpio=`/`uart=` link.
pub const GRAPH_COMPONENT_WITHOUT_LINK: DiagCode = DiagCode::new("E0405");
/// A device without an id.
pub const GRAPH_DEVICE_WITHOUT_ID: DiagCode = DiagCode::new("E0406");
/// A device without its component mapping.
pub const GRAPH_DEVICE_WITHOUT_MAPPING: DiagCode = DiagCode::new("E0407");
/// A pin attribute that is not a 0..=255 number.
pub const GRAPH_INVALID_PIN: DiagCode = DiagCode::new("E0408");

/// A peripheral tag `build` does not lower yet.
pub const PERIPHERAL_UNSUPPORTED: DiagCode = DiagCode::new("E0501");
/// `<gpio pin>` that is not a number.
pub const GPIO_INVALID_PIN: DiagCode = DiagCode::new("E0502");
/// `<gpio direction>` outside `input`/`output`.
pub const GPIO_INVALID_DIRECTION: DiagCode = DiagCode::new("E0503");
/// An `<i2c>` id other than i2c0..i2c2.
pub const I2C_INVALID_ID: DiagCode = DiagCode::new("E0504");
/// An `<i2c>` pin that is not a number.
pub const I2C_INVALID_PIN: DiagCode = DiagCode::new("E0505");
/// An `<i2c frequency>` that is not `NkHz`.
pub const I2C_INVALID_FREQUENCY: DiagCode = DiagCode::new("E0506");
/// An `<spi>` id other than spi2.
pub const SPI_INVALID_ID: DiagCode = DiagCode::new("E0507");
/// An `<spi>` pin that is not a number.
pub const SPI_INVALID_PIN: DiagCode = DiagCode::new("E0508");
/// An `<spi mode>` outside 0..=3.
pub const SPI_INVALID_MODE: DiagCode = DiagCode::new("E0509");
/// An `<spi frequency>` that is not `NkHz`.
pub const SPI_INVALID_FREQUENCY: DiagCode = DiagCode::new("E0510");
/// A `<uart>` id other than uart0..uart3.
pub const UART_INVALID_ID: DiagCode = DiagCode::new("E0511");
/// An explicit `<uart uart>` outside UART0..UART3.
pub const UART_INVALID_PERIPHERAL: DiagCode = DiagCode::new("E0512");
/// A `<uart>` pin that is not a number.
pub const UART_INVALID_PIN: DiagCode = DiagCode::new("E0513");

/// An `<led>` that cannot be built.
pub const COMPONENT_INVALID_LED: DiagCode = DiagCode::new("E0601");
/// A `<button>` that cannot be built.
pub const COMPONENT_INVALID_BUTTON: DiagCode = DiagCode::new("E0602");
/// An `<i2c_component>` that cannot be built.
pub const COMPONENT_INVALID_I2C: DiagCode = DiagCode::new("E0603");
/// An `<spi_component>` that cannot be built.
pub const COMPONENT_INVALID_SPI: DiagCode = DiagCode::new("E0604");
/// A `<uart_component>` that cannot be built.
pub const COMPONENT_INVALID_UART: DiagCode = DiagCode::new("E0605");
/// A component tag `build` does not lower yet.
pub const COMPONENT_UNSUPPORTED: DiagCode = DiagCode::new("E0606");
/// A chip-select attribute that is neither a pin nor a gpio id.
pub const COMPONENT_INVALID_CS: DiagCode = DiagCode::new("E0607");
/// A `<led>`/`<button>` referencing a gpio id that does not exist.
pub const COMPONENT_UNKNOWN_GPIO: DiagCode = DiagCode::new("E0608");
/// A led on an input pin or a button on an output pin.
pub const COMPONENT_WRONG_DIRECTION: DiagCode = DiagCode::new("E0609");
/// `<uart_component baud_rate="0">`.
pub const COMPONENT_INVALID_BAUD: DiagCode = DiagCode::new("E0610");
/// Two `<i2c_component>`s on one `<i2c>`.
pub const COMPONENT_I2C_BUS_USED: DiagCode = DiagCode::new("E0611");
/// Two `<uart_component>`s on one `<uart>`.
pub const COMPONENT_UART_USED: DiagCode = DiagCode::new("E0612");
/// A component pointing at a bus that is not a bus of that kind.
pub const COMPONENT_UNKNOWN_BUS: DiagCode = DiagCode::new("E0613");
/// A component id that cannot become a Rust identifier.
pub const COMPONENT_INVALID_ID: DiagCode = DiagCode::new("E0614");

/// Two elements on one pin.
pub const CONFLICT_PIN: DiagCode = DiagCode::new("E0701");
/// A pin claimed by two elements of the same kind.
pub const CONFLICT_PIN_USED: DiagCode = DiagCode::new("E0702");
/// One element needs an input and the other an output.
pub const CONFLICT_DIRECTION: DiagCode = DiagCode::new("E0703");
/// Two elements on one `<uart>` peripheral.
pub const CONFLICT_PERIPHERAL: DiagCode = DiagCode::new("E0704");
/// An `<i2c>` bus whose SDA and SCL are the same pin.
pub const CONFLICT_I2C_WIRING: DiagCode = DiagCode::new("E0705");
/// An `<spi>` bus whose SCLK and MOSI are the same pin.
pub const CONFLICT_SPI_WIRING: DiagCode = DiagCode::new("E0706");
/// A `<uart>` whose TX and RX are the same pin.
pub const CONFLICT_UART_WIRING: DiagCode = DiagCode::new("E0707");

/// `# devices` entries are not lowered yet.
pub const SERVICE_UNSUPPORTED_DEVICES: DiagCode = DiagCode::new("E0801");
/// A service other than `<wifi>`.
pub const SERVICE_UNSUPPORTED: DiagCode = DiagCode::new("E0802");
/// A `<wifi>` field other than network1.ssid/password.
pub const SERVICE_UNSUPPORTED_FIELD: DiagCode = DiagCode::new("E0803");
/// `<wifi>` without an ssid.
pub const SERVICE_MISSING_SSID: DiagCode = DiagCode::new("E0804");
/// An ssid outside 1..=32 bytes.
pub const SERVICE_INVALID_SSID: DiagCode = DiagCode::new("E0805");
/// A password that is neither empty nor 8..=63 characters.
pub const SERVICE_INVALID_PASSWORD: DiagCode = DiagCode::new("E0806");
/// A wifi value that cannot be a Rust string literal.
pub const SERVICE_INVALID_VALUE: DiagCode = DiagCode::new("E0807");
/// A second `<wifi>` in `# services`.
pub const SERVICE_DUPLICATE_WIFI: DiagCode = DiagCode::new("E0808");
/// A device tag with no compile-time driver.
pub const DRIVER_UNKNOWN: DiagCode = DiagCode::new("E0809");

/// The fallback for a message the table does not know.
///
/// Reaching it is a bug: every producer must have an entry above. The JSON
/// contract test (`espforge/tests/check_json.rs`) fails if one reaches output.
pub const UNCLASSIFIED: DiagCode = DiagCode::new("E0999");

/// The code for a command-line answer that never enters the source pipeline.
///
/// The human output for these stays the one-line `❌ ...`; only JSON needs a
/// code. The flag errors reuse the core/option code they belong to.
pub fn for_cli_message(message: &str) -> DiagCode {
    if message.starts_with("unknown chip") {
        return CORE_UNKNOWN_CHIP;
    }
    if message.starts_with("unknown option") {
        return OPTION_UNKNOWN;
    }
    if message.starts_with("--option") || message.contains(" --option ") {
        return OPTION_INVALID_VALUE;
    }
    CLI_ANSWER
}

/// The code for one diagnostic message.
///
/// Titles are stable user-facing strings; the few that carry an id or option
/// name are matched by prefix/suffix.
pub fn for_message(message: &str) -> DiagCode {
    // Exact titles first. This is deliberately a table, not a heuristic:
    // changing a title should force a decision about its code.
    let exact = match message {
        "malformed node line" => SYNTAX_NODE_LINE,
        "invalid section header" => SYNTAX_SECTION_HEADER,
        "# espforge is incomplete" => CORE_INCOMPLETE,
        "<chip> is missing its type" => CORE_CHIP_WITHOUT_TYPE,
        "unknown chip" => CORE_UNKNOWN_CHIP,
        "<runtime> is missing its type" => CORE_RUNTIME_WITHOUT_TYPE,
        "unknown runtime" => CORE_UNKNOWN_RUNTIME,
        "<console> is missing its type" => CORE_CONSOLE_WITHOUT_TYPE,
        "unknown console" => CORE_UNKNOWN_CONSOLE,
        "unknown peripheral" => GRAPH_UNKNOWN_PERIPHERAL,
        "unknown component" => GRAPH_UNKNOWN_COMPONENT,
        "peripheral is missing an id" => GRAPH_PERIPHERAL_WITHOUT_ID,
        "component is missing an id" => GRAPH_COMPONENT_WITHOUT_ID,
        "component is missing its controller link" => GRAPH_COMPONENT_WITHOUT_LINK,
        "device is missing an id" => GRAPH_DEVICE_WITHOUT_ID,
        "device is missing its component mapping" => GRAPH_DEVICE_WITHOUT_MAPPING,
        "invalid pin number" => GRAPH_INVALID_PIN,
        "unsupported peripheral" => PERIPHERAL_UNSUPPORTED,
        "invalid gpio pin" => GPIO_INVALID_PIN,
        "invalid gpio direction" => GPIO_INVALID_DIRECTION,
        "invalid i2c id" => I2C_INVALID_ID,
        "invalid i2c pin" => I2C_INVALID_PIN,
        "invalid i2c frequency" => I2C_INVALID_FREQUENCY,
        "invalid spi id" => SPI_INVALID_ID,
        "invalid spi pin" => SPI_INVALID_PIN,
        "invalid spi mode" => SPI_INVALID_MODE,
        "invalid spi frequency" => SPI_INVALID_FREQUENCY,
        "invalid uart id" => UART_INVALID_ID,
        "invalid uart peripheral" => UART_INVALID_PERIPHERAL,
        "invalid uart pin" => UART_INVALID_PIN,
        "invalid led" => COMPONENT_INVALID_LED,
        "invalid button" => COMPONENT_INVALID_BUTTON,
        "invalid i2c_component" => COMPONENT_INVALID_I2C,
        "invalid spi_component" => COMPONENT_INVALID_SPI,
        "invalid uart_component" => COMPONENT_INVALID_UART,
        "unsupported component" => COMPONENT_UNSUPPORTED,
        "invalid cs" => COMPONENT_INVALID_CS,
        "unknown gpio" => COMPONENT_UNKNOWN_GPIO,
        "wrong gpio direction" => COMPONENT_WRONG_DIRECTION,
        "invalid baud_rate" => COMPONENT_INVALID_BAUD,
        "i2c bus already used" => COMPONENT_I2C_BUS_USED,
        "uart already used" => COMPONENT_UART_USED,
        "pin conflict" => CONFLICT_PIN,
        "pin already used" => CONFLICT_PIN_USED,
        "pin direction clash" => CONFLICT_DIRECTION,
        "peripheral clash" => CONFLICT_PERIPHERAL,
        "i2c wiring" => CONFLICT_I2C_WIRING,
        "spi wiring" => CONFLICT_SPI_WIRING,
        "uart wiring" => CONFLICT_UART_WIRING,
        "unsupported devices" => SERVICE_UNSUPPORTED_DEVICES,
        "unsupported service" => SERVICE_UNSUPPORTED,
        "unsupported wifi field" => SERVICE_UNSUPPORTED_FIELD,
        "missing wifi ssid" => SERVICE_MISSING_SSID,
        "invalid wifi ssid" => SERVICE_INVALID_SSID,
        "invalid wifi password" => SERVICE_INVALID_PASSWORD,
        "invalid wifi value" => SERVICE_INVALID_VALUE,
        "duplicate wifi" => SERVICE_DUPLICATE_WIFI,
        _ => return dynamic_code(message),
    };
    exact
}

/// Titles that embed an id, key or section name.
fn dynamic_code(message: &str) -> DiagCode {
    if message.starts_with("duplicate section `") {
        return STRUCTURE_DUPLICATE_SECTION;
    }
    if message.starts_with("missing section `") {
        return STRUCTURE_MISSING_SECTION;
    }
    if message.starts_with("unknown element <") {
        return CORE_UNKNOWN_ELEMENT;
    }
    if message.starts_with("duplicate <") {
        return CORE_DUPLICATE_ELEMENT;
    }
    if message.starts_with("unknown option `") {
        return OPTION_UNKNOWN;
    }
    if message.ends_with("has an invalid value") && message.starts_with('`') {
        return OPTION_INVALID_VALUE;
    }
    if message.ends_with("has the wrong type") && message.starts_with('`') {
        return VALUE_WRONG_TYPE;
    }
    if message.ends_with("is not a boolean") && message.starts_with('`') {
        return VALUE_NOT_BOOLEAN;
    }
    if message.ends_with("is not a number") && message.starts_with('`') {
        return VALUE_NOT_NUMBER;
    }
    if message.ends_with("and an object") && message.contains("is both a") {
        return VALUE_KEY_KIND;
    }
    if message.starts_with("missing required attribute `") {
        return VALUE_MISSING_ATTRIBUTE;
    }
    if message.ends_with("cannot be set in <options>") && message.starts_with('`') {
        return OPTION_DERIVED;
    }
    if message.ends_with("is not available for") || message.contains("is not available for ") {
        return OPTION_UNAVAILABLE;
    }
    if message.contains(" requires `") && message.starts_with('`') {
        return OPTION_REQUIRES;
    }
    if message.starts_with("options ") && message.ends_with("are mutually exclusive") {
        return OPTION_MUTUALLY_EXCLUSIVE;
    }
    if message.ends_with("has no USB-Serial-JTAG") {
        return OPTION_NO_CONSOLE;
    }
    if message.starts_with("unknown ") && message.ends_with(" bus") {
        return COMPONENT_UNKNOWN_BUS;
    }
    if message.starts_with("invalid ") && message.ends_with("") {
        // `invalid {label}` from require_bus; the exact labels are in the
        // table above, so this catches the generic spelling.
        if message == "invalid gpio" || message == "invalid i2c" || message == "invalid spi" {
            return COMPONENT_INVALID_ID;
        }
    }
    UNCLASSIFIED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_title_has_its_own_code() {
        let titles = [
            "malformed node line",
            "invalid section header",
            "# espforge is incomplete",
            "<chip> is missing its type",
            "unknown chip",
            "<runtime> is missing its type",
            "unknown runtime",
            "<console> is missing its type",
            "unknown console",
            "unknown option `wombat`",
            "`alloc` has an invalid value",
            "`alloc` cannot be set in <options>",
            "`wifi` requires `alloc`",
            "`wifi` is not available for esp32c3",
            "options `a`, `b` are mutually exclusive",
            "esp32c3 has no USB-Serial-JTAG",
            "`network1.ssid` has the wrong type",
            "`x` is not a boolean",
            "`x` is not a number",
            "`x` is both a string and an object",
            "missing required attribute `pin`",
            "unknown peripheral",
            "unknown component",
            "peripheral is missing an id",
            "component is missing an id",
            "component is missing its controller link",
            "device is missing an id",
            "device is missing its component mapping",
            "invalid pin number",
            "unsupported peripheral",
            "invalid gpio pin",
            "invalid gpio direction",
            "invalid i2c id",
            "invalid i2c pin",
            "invalid i2c frequency",
            "invalid spi id",
            "invalid spi pin",
            "invalid spi mode",
            "invalid spi frequency",
            "invalid uart id",
            "invalid uart peripheral",
            "invalid uart pin",
            "invalid led",
            "invalid button",
            "invalid i2c_component",
            "invalid spi_component",
            "invalid uart_component",
            "unsupported component",
            "invalid cs",
            "unknown gpio",
            "wrong gpio direction",
            "invalid baud_rate",
            "i2c bus already used",
            "uart already used",
            "unknown i2c bus",
            "pin conflict",
            "pin already used",
            "pin direction clash",
            "peripheral clash",
            "i2c wiring",
            "spi wiring",
            "uart wiring",
            "unsupported devices",
            "unsupported service",
            "unsupported wifi field",
            "missing wifi ssid",
            "invalid wifi ssid",
            "invalid wifi password",
            "invalid wifi value",
            "duplicate wifi",
            "duplicate section `# peripherals`",
            "missing section `# services`",
            "unknown element <pwm> in # espforge",
            "duplicate <chip> in # espforge",
        ];

        let mut seen = std::collections::BTreeSet::new();
        for title in titles {
            let code = for_message(title);
            assert_ne!(code, UNCLASSIFIED, "{title} fell through");
            assert!(
                seen.insert(code.as_str()),
                "{title} reuses code {}",
                code.as_str()
            );
        }
    }

    #[test]
    fn an_unknown_title_is_not_silently_given_a_real_code() {
        assert_eq!(for_message("a message nobody wrote"), UNCLASSIFIED);
    }
}
