//! The typed, parsed form of the user's YAML project (ADR-004).
//!
//! This is the output of the `parse` stage and the input to `validate`. It is
//! deliberately close to the YAML shape (sections + `$name` refs) but fully
//! typed: `using` selects a driver by name, `with` carries the driver's
//! properties, and references are normalized `ResourceRef`/`PinRef` value
//! objects rather than raw strings.

use crate::value::{PinRef, ResourceRef, Span};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A fully parsed project. The single source-of-truth description before
/// validation/resolution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Project {
    pub espforge: EspforgeMeta,
    pub esp32: Esp32Section,
    #[serde(default, deserialize_with = "instance_list")]
    pub components: Vec<Instance>,
    #[serde(default, deserialize_with = "instance_list")]
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
///
/// Named-map shape (v1 heritage, per design §17.4): each resource is keyed by an
/// explicit name the user chooses, e.g. `gpio2: { pin: 18, direction: output }`.
/// That name is the `$ref` source — `components` reference it as `$gpio2`, never
/// by GPIO number (explicit over implicit, Zen of espforge).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Esp32Section {
    #[serde(default)]
    pub gpio: HashMap<String, GpioPin>,
    #[serde(default)]
    pub i2c: HashMap<String, I2cConfig>,
    #[serde(default)]
    pub spi: HashMap<String, SpiConfig>,
    #[serde(default)]
    pub uart: HashMap<String, UartConfig>,
    #[serde(default)]
    pub wifi: Option<WifiConfig>,
    #[serde(default)]
    pub psram: Option<PsRamConfig>,
    #[serde(default)]
    pub heap: Option<HeapConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpioPin {
    /// GPIO number. Aliased from `number`.
    #[serde(alias = "number")]
    pub pin: u32,
    /// Signal direction at the pad. Accepted and echoed; not yet enforced.
    #[serde(default)]
    pub direction: Option<Direction>,
}

/// Pad direction (v1 spelling). Carried into the IR; enforcement is a later
/// `validate` enhancement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Input,
    Output,
}

/// I2C bus configuration (`esp32.i2c`).
///
/// `deny_unknown_fields` so a typo like `scll` is rejected at parse time —
/// this is what lets the future VS Code addon validate the YAML structurally.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct I2cConfig {
    /// Hardware peripheral index, e.g. `0` for `I2C0`. YAML key `i2c`.
    #[serde(alias = "i2c")]
    pub peripheral: u32,
    #[serde(default)]
    pub sda: Option<u32>,
    #[serde(default)]
    pub scl: Option<u32>,
    /// Bus clock frequency in kHz.
    #[serde(default, rename = "frequency_kHz", alias = "frequency")]
    pub frequency_khz: Option<u32>,
}

/// SPI bus configuration (`esp32.spi`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpiConfig {
    /// Hardware peripheral index, e.g. `2` for `SPI2`. YAML key `spi`.
    #[serde(alias = "spi")]
    pub peripheral: u32,
    #[serde(default)]
    pub mosi: Option<u32>,
    #[serde(default)]
    pub miso: Option<u32>,
    /// Clock pin. Accepts `sclk` or `sck` (Wokwi/v1 spelling).
    #[serde(default, alias = "sck")]
    pub sclk: Option<u32>,
    /// Chip-select pin (managed by the runtime as an `Output`).
    #[serde(default)]
    pub cs: Option<u32>,
    /// SPI mode (0–3).
    #[serde(default)]
    pub mode: Option<u8>,
    /// Bus clock frequency in kHz.
    #[serde(default, rename = "frequency_kHz", alias = "frequency")]
    pub frequency_khz: Option<u32>,
}

/// UART configuration (`esp32.uart`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UartConfig {
    /// Hardware peripheral index, e.g. `1` for `UART1`. YAML key `uart`.
    #[serde(alias = "uart")]
    pub peripheral: u32,
    #[serde(default)]
    pub tx: Option<u32>,
    #[serde(default)]
    pub rx: Option<u32>,
    /// Baud rate. Accepts `baudrate` (preferred, design §20.6), or the legacy
    /// aliases `baud` / `frequency_kHz`.
    #[serde(default, alias = "baudrate", alias = "baud", rename = "frequency_kHz")]
    pub baud: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: String,
    /// Authentication mode, e.g. `open`, `wpa2`. Optional; defaults to `open`.
    #[serde(default = "default_auth")]
    pub auth: String,
}

fn default_auth() -> String {
    "open".to_string()
}

/// PSRAM configuration (schema-complete per §17.4; consumed when a driver needs
/// it). `mode` is the esp-hal PSRAM mode, e.g. `octal`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsRamConfig {
    pub mode: String,
}

/// Heap configuration (schema-complete per §17.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeapConfig {
    pub size: u32,
}

/// A named occurrence of a component or device (ADR-003: an `Instance`).
///
/// `using` selects the driver; `with` is the driver-specific property map,
/// deserialized into the driver's typed schema at validate time (ADR-004).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// The instance's name (the `$name` others reference). Aliased from `name`.
    /// Defaults to empty so the v1 keyed-map form can backfill it from the map
    /// key (see [`InstanceList`]).
    #[serde(default, alias = "name")]
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

/// A `Vec<Instance>` that deserializes from either form:
/// - the v2 sequence form:  `[ { id: red_led, using: led, with: ... }, ... ]`
/// - the v1 keyed-map form: `red_led: { using: led, with: ... }` (the map key
///   becomes the instance `id`). The map form is the "good bit" carried over
/// from v1; both forms coexist (non-breaking).
#[derive(Debug, Clone, Default, Serialize)]
pub struct InstanceList(pub Vec<Instance>);

impl<'de> Deserialize<'de> for InstanceList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let value = serde_yaml_ng::Value::deserialize(deserializer)?;
        match value {
            // v2 sequence form.
            serde_yaml_ng::Value::Sequence(seq) => {
                let mut out = Vec::with_capacity(seq.len());
                for item in seq {
                    out.push(
                        Instance::deserialize(item)
                            .map_err(|e: serde_yaml_ng::Error| D::Error::custom(e.to_string()))?,
                    );
                }
                Ok(InstanceList(out))
            }
            // v1 keyed-map form.
            serde_yaml_ng::Value::Mapping(map) => {
                let mut out = Vec::with_capacity(map.len());
                for (key, val) in map {
                    let id = match key {
                        serde_yaml_ng::Value::String(s) => s,
                        other => {
                            return Err(D::Error::custom(format!(
                                "component/device key must be a string, found {other:?}"
                            )))
                        }
                    };
                    let mut inst = Instance::deserialize(val)
                        .map_err(|e: serde_yaml_ng::Error| D::Error::custom(e.to_string()))?;
                    // The map key is the instance id; only fill it if the value
                    // did not already specify one.
                    if inst.id.is_empty() {
                        inst.id = id;
                    }
                    out.push(inst);
                }
                Ok(InstanceList(out))
            }
            serde_yaml_ng::Value::Null => Ok(InstanceList(Vec::new())),
            other => Err(D::Error::custom(format!(
                "components/devices must be a list or a map, found {other:?}"
            ))),
        }
    }
}

/// `deserialize_with` target for `Project::components`/`devices`: accepts the
/// v2 sequence or v1 keyed-map form and yields `Vec<Instance>`.
fn instance_list<'de, D>(deserializer: D) -> Result<Vec<Instance>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(InstanceList::deserialize(deserializer)?.0)
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
