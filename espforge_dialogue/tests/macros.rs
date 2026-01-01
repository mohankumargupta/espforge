use espforge_dialogue::{Asker, EnumAsker};

#[derive(Asker)]
pub struct TestConfig {
    #[input]
    pub name: String,
    #[confirm]
    pub debug: bool,
    #[input]
    pub description: Option<String>,
}

#[derive(Asker)]
pub struct PromptConfig {
    #[input(prompt = "What is your name?")]
    pub name: String,
    #[confirm(prompt = "Are you sure?")]
    pub sure: bool,
}

#[derive(EnumAsker, Debug, PartialEq)]
#[asker(prompt = "Select a mode")]
pub enum TestMode {
    #[asker(label = "Fast Mode")]
    Fast,
    #[asker(label = "Safe Mode")]
    Safe,
}


#[derive(EnumAsker, Debug, PartialEq)]
#[asker(prompt = "Select Target Chip")]
pub enum Chip {
    #[asker(label = "esp32c3")]
    Esp32c3,
    #[asker(label = "esp32c6")]
    Esp32c6,
    #[asker(label = "esp32s3")]
    Esp32s3,
    #[asker(label = "esp32s2")]
    Esp32s2,
    #[asker(label = "esp32")]
    Esp32,
    #[asker(label = "esp32h2")]
    Esp32h2,
}

#[derive(EnumAsker, Debug, PartialEq)]
#[asker(prompt = "Select Development Board")]
pub enum Board {
    #[asker(label = "Espressif ESP32-C3-DevKitM-1", group = "esp32c3")]
    Esp32C3DevKitM1,
    
    #[asker(label = "Seeed Studio XIAO ESP32C3", group = "esp32c3")]
    XiaoEsp32C3,

    #[asker(label = "Espressif ESP32-S3-DevKitC-1", group = "esp32s3")]
    Esp32S3DevKitC1,
    
    #[asker(label = "LILYGO T-Display-S3", group = "esp32s3")]
    LilyGoTDisplayS3,

    #[asker(label = "Generic Board")]
    Generic,
}



#[test]
fn test_asker_struct_generation() {
    let builder = TestConfig::asker();
    // Verify methods check (compile-time)
    let _check_methods = || {
        let _ = builder
            .name("Enter name")
            .debug("Enable debug?")
            .description("Enter description")
            .finish();
    };
}

#[test]
fn test_asker_prompt_generation() {
    let builder = PromptConfig::asker();
    // Verify methods take NO arguments because prompts are defined in attributes
    let _check_methods = || {
        let _ = builder
            .name()
            .sure()
            .finish();
    };
}

#[test]
fn test_enum_asker_generation() {
    let _ask_fn: fn() -> TestMode = TestMode::ask;
}


#[test]
fn test_chip_asker_api() {
    // Verify that the simple `ask` method is generated with the correct signature
    let _ask_fn: fn() -> Chip = Chip::ask;
}

#[test]
fn test_board_asker_api() {
    let _ask_filtered_fn: fn(&str) -> Board = Board::ask_filtered;
    let _ask_fn: fn() -> Board = Board::ask;
}

