use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct Board {
    pub id: String,
    pub name: String,
    #[serde(rename = "gnd_top_left")]
    pub gnd_top_left: String,
    #[serde(rename = "gnd_top_right")]
    pub gnd_top_right: String,
    #[serde(rename = "gnd_bottom_left")]
    pub gnd_bottom_left: String,
    #[serde(rename = "gnd_bottom_right")]
    pub gnd_bottom_right: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Chip {
    pub id: String,
    #[serde(rename = "wokwi_board_id")]
    pub wokwi_board_id: String,
    #[serde(rename = "serial_interface")]
    pub serial_interface: String,
    pub max_heap_size: usize,
}

#[derive(Debug, Deserialize)]
struct Chips {
    chip: Vec<Chip>,
}

#[derive(Debug, Deserialize)]
struct Boards {
    board: Vec<Board>,
}

pub struct BoardDatabase {
    chips: HashMap<String, Chip>,
    boards: HashMap<String, Board>,
}

impl BoardDatabase {
    pub fn load() -> Self {
        // Load chips.toml
        let chips_data = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/chips.toml"));
        let chips: Chips = toml::from_str(chips_data).expect("Failed to parse chips.toml");

        // Load boards.toml
        let boards_data = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/boards.toml"));
        let boards: Boards = toml::from_str(boards_data).expect("Failed to parse boards.toml");

        let chip_map: HashMap<String, Chip> = chips
            .chip
            .into_iter()
            .map(|chip| (chip.id.clone(), chip))
            .collect();

        let board_map: HashMap<String, Board> = boards
            .board
            .into_iter()
            .map(|board| (board.id.clone(), board))
            .collect();

        Self {
            chips: chip_map,
            boards: board_map,
        }
    }

    /// Returns the Wokwi board ID/type (e.g., "board-esp32-devkit-c-v4") for the given chip
    /// This is the value that should be used in diagram.json's "type" field
    pub fn wokwi_board(&self, chip_id: &str) -> Option<String> {
        self.chips
            .get(chip_id)
            .map(|chip| chip.wokwi_board_id.clone())
    }

    /// Returns the human-readable board name for the given chip
    pub fn board_name(&self, chip_id: &str) -> Option<String> {
        let board_id = self.wokwi_board(chip_id)?;
        self.boards.get(&board_id).map(|board| board.name.clone())
    }

    pub fn gnd_top_left(&self, chip_id: &str) -> Option<String> {
        let board_id = self.wokwi_board(chip_id)?;
        self.boards
            .get(&board_id)
            .map(|board| board.gnd_top_left.clone())
    }

    pub fn gnd_top_right(&self, chip_id: &str) -> Option<String> {
        let board_id = self.wokwi_board(chip_id)?;
        self.boards
            .get(&board_id)
            .map(|board| board.gnd_top_right.clone())
    }

    pub fn gnd_bottom_left(&self, chip_id: &str) -> Option<String> {
        let board_id = self.wokwi_board(chip_id)?;
        self.boards
            .get(&board_id)
            .map(|board| board.gnd_bottom_left.clone())
    }

    pub fn gnd_bottom_right(&self, chip_id: &str) -> Option<String> {
        let board_id = self.wokwi_board(chip_id)?;
        self.boards
            .get(&board_id)
            .map(|board| board.gnd_bottom_right.clone())
    }

    pub fn is_valid_chip(&self, chip_id: &str) -> bool {
        self.chips.contains_key(chip_id)
    }

    pub fn all_chips(&self) -> Vec<String> {
        self.chips.keys().cloned().collect()
    }

    /// Get maximum heap size for a chip
    pub fn max_heap_size(&self, chip_id: &str) -> Option<usize> {
        self.chips.get(chip_id).map(|chip| chip.max_heap_size)
    }
}
