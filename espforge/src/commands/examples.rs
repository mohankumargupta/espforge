//! `espforge examples`: the embedded catalog, grouped by category, with the
//! title each example carries and the command that turns one into a project.
//!
//! Pure printing: the catalog and its order come from `espforge_examples`.

use std::process::ExitCode;

use espforge_examples::ExampleInfo;

pub fn run() -> ExitCode {
    print!("{}", render(&espforge_examples::list()));
    ExitCode::SUCCESS
}

/// Render the catalog: a category line, then `  name  title` per example.
pub fn render(list: &[ExampleInfo]) -> String {
    let width = list.iter().map(|e| e.name.len()).max().unwrap_or(0);
    let mut out = String::new();
    let mut current: Option<&str> = None;
    for example in list {
        if current != Some(example.category.as_str()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&example.category);
            out.push('\n');
            current = Some(example.category.as_str());
        }
        let line = format!(
            "  {:<width$}  {}",
            example.name,
            example.title.as_deref().unwrap_or("")
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str("\nCreate a project:  espforge new <example> <dir>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(category: &str, name: &str, title: Option<&str>) -> ExampleInfo {
        ExampleInfo {
            key: format!("{category}/{name}"),
            category: category.into(),
            name: name.into(),
            title: title.map(str::to_string),
        }
    }

    #[test]
    fn render_groups_categories_and_aligns_titles() {
        let list = vec![
            info("01.Basics", "blink", Some("BLINK EXAMPLE")),
            info("01.Basics", "blink_async", Some("ASYNC BLINK EXAMPLE")),
            info("02.Digital", "button", None),
        ];
        assert_eq!(
            render(&list),
            "01.Basics\n\
             \x20 blink        BLINK EXAMPLE\n\
             \x20 blink_async  ASYNC BLINK EXAMPLE\n\
             \n\
             02.Digital\n\
             \x20 button\n\
             \n\
             Create a project:  espforge new <example> <dir>\n"
        );
    }

    #[test]
    fn renders_every_catalog_example() {
        let list = espforge_examples::list();
        let text = render(&list);
        assert!(text.starts_with("01.Basics\n"), "{text}");
        for example in &list {
            assert!(
                text.contains(&format!("  {}", example.name)),
                "missing {} in:\n{text}",
                example.key
            );
            for line in text.lines() {
                assert!(!line.contains(&example.key), "list only shows short names");
            }
        }
        assert!(
            text.trim_end()
                .ends_with("Create a project:  espforge new <example> <dir>"),
            "{text}"
        );
    }
}
