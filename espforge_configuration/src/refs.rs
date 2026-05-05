use serde::de::{self, Deserializer};
use std::marker::PhantomData;

/// Marker type for a reference to a named component (e.g. `SpiDevice`, `I2cDevice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentRef;

/// Marker type for a reference to a GPIO pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinRef;

/// A typed YAML reference to a hardware resource.
///
/// The leading `$` prefix (e.g. `$spi2`) is stripped during deserialization
/// so callers never need to handle normalization manually.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRef<T> {
    raw: String,
    _kind: PhantomData<T>,
}

impl<T> DeviceRef<T> {
    /// Returns the normalized name without the `$` prefix.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl<T> AsRef<str> for DeviceRef<T> {
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

impl<'de, T> serde::Deserialize<'de> for DeviceRef<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let normalized = value.trim().strip_prefix('$').unwrap_or(value.trim());
        if normalized.is_empty() {
            return Err(de::Error::custom(
                "Device reference cannot be empty after normalization",
            ));
        }
        Ok(Self {
            raw: normalized.to_string(),
            _kind: PhantomData,
        })
    }
}

impl<T> std::fmt::Display for DeviceRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}
