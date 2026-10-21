//! Typed wrappers for the ESPF `<*_component>` bus components.
//!
//! Each module wraps one component tag from the ESPF schema
//! (see `examples/example.espf.txt`):
//!
//! - [`gpio_component`] — `<gpio_component id.. gpio="$gpioX" direction="output" />`
//! - [`i2c_component`]  — `<i2c_component id.. i2c="$i2cN" address="0x.." />`
//! - [`spi_component`]  — `<spi_component id.. spi="$spiN" mode="0" cs="$gpioX" />`
//! - [`uart_component`] — `<uart_component id.. uart="$uartN" baud_rate="9600" />`
//!
//! Each wrapper exposes a `resolve(node, ctx) -> Result<Self, String>`
//! constructor that validates required attributes and resolves the
//! `$`-parent-peripheral reference through the [`RuntimeContext`] tables.

pub mod gpio_component;
pub mod i2c_component;
pub mod spi_component;
pub mod uart_component;

/// The component tags taken by [`resolve_component`], so the `espforge espf`
/// catalog can be drift-checked against the wrappers.
pub const TAGS: &[&str] = &["gpio_component", "i2c_component", "spi_component", "uart_component"];

/// The required-attribute list of the wrapper named by `tag` (each wrapper
/// keeps it next to its `resolve`).
pub fn resolve_tag_required_attrs(tag: &str) -> Option<Vec<&'static str>> {
    match tag {
        "gpio_component" => Some(gpio_component::REQUIRED_ATTRS.to_vec()),
        "i2c_component" => Some(i2c_component::REQUIRED_ATTRS.to_vec()),
        "spi_component" => Some(spi_component::REQUIRED_ATTRS.to_vec()),
        "uart_component" => Some(uart_component::REQUIRED_ATTRS.to_vec()),
        _ => None,
    }
}

pub use gpio_component::{GpioComponent, GpioDirection};
pub use i2c_component::{I2cComponent, parse_address as parse_i2c_address};
pub use spi_component::SpiComponent;
pub use uart_component::{Parity, StopBits, UartComponent};

use espf::context::{ParsedNode, RuntimeContext};

/// Resolves any `<*_component>` node to its typed wrapper by tag name.
pub fn resolve_component<'a>(
    node: &'a ParsedNode<'a>,
    ctx: &RuntimeContext<'a>,
) -> Result<Component<'a>, String> {
    match node.tag_name {
        "gpio_component" => Ok(Component::Gpio(GpioComponent::resolve(node, ctx)?)),
        "i2c_component" => Ok(Component::I2c(I2cComponent::resolve(node, ctx)?)),
        "spi_component" => Ok(Component::Spi(SpiComponent::resolve(node, ctx)?)),
        "uart_component" => Ok(Component::Uart(UartComponent::resolve(node, ctx)?)),
        other => Err(format!("❌ Unknown component tag <{other}>")),
    }
}

/// Any typed bus component.
#[derive(Debug, Clone)]
pub enum Component<'a> {
    Gpio(GpioComponent<'a>),
    I2c(I2cComponent<'a>),
    Spi(SpiComponent<'a>),
    Uart(UartComponent<'a>),
}

impl Component<'_> {
    /// The component's `id` attribute value.
    pub fn id(&self) -> &str {
        match self {
            Component::Gpio(c) => c.id,
            Component::I2c(c) => c.id,
            Component::Spi(c) => c.id,
            Component::Uart(c) => c.id,
        }
    }

    /// Handle of the resolved parent peripheral.
    pub fn peripheral_handle(&self) -> espf::context::Handle {
        match self {
            Component::Gpio(c) => c.peripheral_handle,
            Component::I2c(c) => c.peripheral_handle,
            Component::Spi(c) => c.peripheral_handle,
            Component::Uart(c) => c.peripheral_handle,
        }
    }
}
