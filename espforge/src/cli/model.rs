pub struct ExampleConfig {
    pub template_name: String,
    pub project_name: String,
    pub chip: String,
}

pub struct ExportResult {
    pub project_name: String,
    pub output_file: String,
}

pub struct ExportOptions {
    pub example_name: String,
    pub override_project_name: Option<String>,
    pub override_platform: Option<String>,
}
