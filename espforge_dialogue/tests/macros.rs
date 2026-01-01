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
#[asker(prompt = "Select Board")]
pub enum TestBoard {
    #[asker(label = "Board A", group = "g1")]
    BoardA,
    #[asker(label = "Board B", group = "g2")]
    BoardB,
    #[asker(label = "Generic")]
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
fn test_enum_asker_filtered_generation() {
    // Verify the filtered ask method exists with correct signature
    let _ask_filtered: fn(&str) -> TestBoard = TestBoard::ask_filtered;
}

