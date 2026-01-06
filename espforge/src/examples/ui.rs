use crate::cli::model::ExportResult;

pub struct ResultPrinter;

impl ResultPrinter {
    pub fn display_success(result: &ExportResult) {
        println!(
            "\n✨ Success! Project initialized in '{}'",
            result.project_name
        );
        println!("To compile the project:");
        println!("  cd {}", result.project_name);
        println!("  espforge compile {}", result.output_file);
    }
}
