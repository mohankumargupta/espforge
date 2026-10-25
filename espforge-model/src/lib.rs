pub mod board;

pub use board::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Blocking,
    Embassy,
}

/// Where `print!`/log output goes (esp-println's backend).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Console {
    /// Plain UART. Works on every chip.
    #[default]
    Uart,
    /// Built-in USB-Serial-JTAG peripheral (not on the original ESP32).
    JtagSerial,
}

/// Contents of the `# espforge` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub name: String,
    pub description: Option<String>,
    pub chip: String,
    pub runtime: Runtime,
    pub console: Console,
}

impl Meta {
    /// The project name as a valid crate/package identifier
    /// (`"example project"` -> `example_project`). Used by both scaffold and emit.
    pub fn crate_name(&self) -> String {
        let slug: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let slug = slug.trim_matches('_').to_string();
        if slug.is_empty() {
            "espforge_project".to_string()
        } else if slug.starts_with(|c: char| c.is_ascii_digit()) {
            format!("p_{slug}")
        } else {
            slug
        }
    }
}

/// Cross-cutting switches derived from the project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub is_embassy: bool,
    pub has_alloc: bool,
    pub has_wifi: bool,
}

/// Everything the emitters need. Instances (components/devices) come later.
#[derive(Debug, Clone)]
pub struct Project {
    pub meta: Meta,
    pub flags: Flags,
}

impl Project {
    pub fn new(meta: Meta) -> Self {
        let flags = Flags {
            is_embassy: meta.runtime == Runtime::Embassy,
            ..Flags::default()
        };
        Self { meta, flags }
    }
}

/// An in-memory generated text file. Emitters return these; a thin I/O layer writes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Path relative to the output dir, e.g. `src/generated.rs`.
    pub path: String,
    pub content: String,
    pub ownership: Ownership,
}

/// An in-memory binary file (e.g. a prebuilt Wokwi `chip.wasm`), copied verbatim.
/// Assets are never tracked in the drift manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// Path relative to the output dir.
    pub path: String,
    pub bytes: Vec<u8>,
    pub ownership: Ownership,
}

impl Asset {
    pub fn seed_once(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
            ownership: Ownership::SeedOnce,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// Regenerated every build and tracked in the manifest (drift detection).
    Owned,
    /// Written only if absent; never tracked, never clobbered.
    SeedOnce,
    /// Not written by espforge at all.
    NotOwned,
}

impl Artifact {
    pub fn owned(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
            ownership: Ownership::Owned,
        }
    }
    pub fn seed_once(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
            ownership: Ownership::SeedOnce,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(name: &str) -> Meta {
        Meta {
            name: name.into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::default(),
        }
    }

    #[test]
    fn crate_name_is_slugged() {
        assert_eq!(meta("example project").crate_name(), "example_project");
        assert_eq!(meta("Hello-World").crate_name(), "hello_world");
        assert_eq!(meta("3d").crate_name(), "p_3d");
        assert_eq!(meta("!!!").crate_name(), "espforge_project");
    }

    #[test]
    fn console_defaults_to_uart() {
        assert_eq!(Console::default(), Console::Uart);
    }
}
