//! The typed, parsed form of the user's YAML project (ADR-004).
//!
//! This is the output of the `parse` stage and the input to `validate`. It is
//! deliberately close to the YAML shape (sections + `$name` refs) but fully
//! typed: `using` selects a driver by name, `with` carries the driver's
//! properties, and references are normalized `ResourceRef`/`PinRef` value
//! objects rather than raw strings.

use crate::value::{PinRef, ResourceRef, Span};
use serde::{Deserialize, Serialize};

/// A fully parsed project. The single source-of-truth description before
/// validation/resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Project {
    pub espforge: EspforgeMeta,
    pub esp32: Esp32Section,
    pub components: Vec<Instance>,
    pub devices: Vec<Instance>,
}

/// Top-level `espforge:` metadata section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EspforgeMeta {
    #[serde(default)]
    pub name: Option<String>,
    /// Target chip, e.g. `esp32`, `esp32c3`. Aliased from `chip`/`platform`.
    #[serde(alias = "chip", alias = "platform")]
    pub target: Option<String>,
    /// Runtime: `blocking` or `embassy`.
    #[serde(default)]
    pub runtime: Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Blocking,
    Embassy,
}

impl Default for Runtime {
    fn default() -> Self {
        Runtime::Blocking
    }
}

/// The `esp32:` section: raw hardware peripherals (ADR-003).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Esp32Section {
    #[serde(default)]
    pub gpio: Vec<GpioPin>,
    #[serde(default)]
    pub i2c: Vec<BusConfig>,
    #[serde(default)]
    pub spi: Vec<BusConfig>,
    #[serde(default)]
    pub uart: Vec<BusConfig>,
    #[serde(default)]
    pub wifi: Option<WifiConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpioPin {
    /// GPIO number. Aliased from `pin`/`number`.
    #[serde(alias = "pin", alias = "number")]
    pub gpio: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusConfig {
    /// Logical bus name, e.g. `i2c0`. The `$name` users reference.
    pub name: String,
    /// Hardware peripheral index, e.g. `0` for `I2C0`.
    pub peripheral: u32,
    #[serde(default)]
    pub sda: Option<u32>,
    #[serde(default)]
    pub scl: Option<u32>,
    #[serde(default)]
    pub mosi: Option<u32>,
    #[serde(default)]
    pub miso: Option<u32>,
    #[serde(default)]
    pub sclk: Option<u32>,
    /// Bus clock frequency in kHz.
    #[serde(default)]
    pub frequency: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: String,
}

/// A named occurrence of a component or device (ADR-003: an `Instance`).
///
/// `using` selects the driver; `with` is the driver-specific property map,
/// deserialized into the driver's typed schema at validate time (ADR-004).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// The instance's name (the `$name` others reference). Aliased from `name`.
    #[serde(alias = "name")]
    pub id: String,
    /// The driver kind, e.g. `led`, `ssd1306`, `http`.
    #[serde(alias = "driver", alias = "using")]
    pub kind: String,
    /// Driver-specific properties (validated against the driver schema later).
    #[serde(default)]
    pub with: serde_yaml_ng::Value,
    /// Source span of this instance node (for diagnostics).
    #[serde(skip)]
    pub span: Span,
}

// --- Reference normalization ------------------------------------------------
//
// `$name` in YAML is normalized to `ResourceRef`/`PinRef` at deserialization so
// downstream code never handles the sigil (ADR-004). `ResourceRef` deserializes
// by stripping a leading `$`.

impl<'de> Deserialize<'de> for ResourceRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let trimmed = raw.trim();
        let name = trimmed
            .strip_prefix('$')
            .unwrap_or(trimmed)
            .trim()
            .to_string();
        if name.is_empty() {
            return Err(serde::de::Error::custom("reference cannot be empty"));
        }
        Ok(ResourceRef::synthetic(name))
    }
}

impl Serialize for ResourceRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("${}", self.name))
    }
}

// `PinRef` can be written in YAML either as an integer (`gpio: 18`) or as a
// `$GPIO18` style ref. We accept the integer form here; the `$` ref form is
// parsed via `ResourceRef` where needed.

impl<'de> Deserialize<'de> for PinRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = serde_yaml_ng::Value::deserialize(deserializer)?;
        match v {
            serde_yaml_ng::Value::Number(n) => {
                let number = n
                    .as_u64()
                    .ok_or_else(|| serde::de::Error::custom("pin number must be an integer"))?;
                Ok(PinRef { number: number as u32, span: Span::default() })
            }
            serde_yaml_ng::Value::String(s) => {
                let trimmed = s.trim();
                let rest = trimmed
                    .strip_prefix('$')
                    .unwrap_or(trimmed)
                    .trim();
                let rest = rest
                    .strip_prefix("GPIO")
                    .or_else(|| rest.strip_prefix("gpio"))
                    .unwrap_or(rest);
                let number = rest
                    .parse::<u32>()
                    .map_err(serde::de::Error::custom)?;
                Ok(PinRef { number, span: Span::default() })
            }
            _ => Err(serde::de::Error::custom("pin must be an integer or GPIO name")),
        }
    }
}

impl Serialize for PinRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("GPIO{}", self.number))
    }
}
