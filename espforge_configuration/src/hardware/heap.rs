use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeapConfig {
    /// Heap size in bytes. Overrides the chip-database default when set.
    pub size: usize,
}

