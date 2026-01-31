use anyhow::Result;
use serde_yaml_ng::Value;

pub trait ConfigParser: Sized {
    fn parse(value: &Value) -> Result<Self>;
}

#[macro_export]
macro_rules! config_struct {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field:ident: $ty:ty $(= $default:expr)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, serde::Deserialize)]
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $(#[serde(default = $default)])?
                pub $field: $ty,
            )*
        }

        impl $crate::ConfigParser for $name {
            fn parse(value: &serde_yaml_ng::Value) -> anyhow::Result<Self> {
                serde_yaml_ng::from_value(value.clone())
                    .context(concat!("Invalid configuration for ", stringify!($name)))
            }
        }
    };
}

// Helper functions for common defaults
pub fn default_false() -> bool {
    false
}
pub fn default_true() -> bool {
    true
}
pub fn default_i2c_addr() -> u8 {
    0x3C
}
