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

#[test]
fn test_asker_struct_generation() {
    let builder = TestConfig::asker();
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

