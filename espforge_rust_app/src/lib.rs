use anyhow::{Result, Context, anyhow};
use std::path::Path;
use std::fs;

pub struct AppCode {
    pub setup: String,
    pub forever: String,
}

pub fn parse_app_rs(path: &Path) -> Result<AppCode> {
    if !path.exists() {
        return Err(anyhow!("app.rs not found at {}", path.display()));
    }

    let content = fs::read_to_string(path)
        .context(format!("Failed to read {}", path.display()))?;

    Ok(AppCode {
        setup: extract_function_body(&content, "setup").unwrap_or_default(),
        forever: extract_function_body(&content, "forever").unwrap_or_default(),
    })
}

fn extract_function_body(source: &str, fn_name: &str) -> Option<String> {
    // Simple parser: find "fn name", find opening brace, count braces until closed
    let search_str = format!("fn {}()", fn_name);
    
    // Find start of function definition
    let fn_start = source.find(&search_str)?;
    
    // Find the opening brace after the function name
    let body_start_idx = source[fn_start..].find('{')? + fn_start;
    
    let mut brace_count = 0;
    let mut body_end_idx = 0;
    let mut found_start = false;

    for (i, char) in source[body_start_idx..].char_indices() {
        match char {
            '{' => {
                brace_count += 1;
                found_start = true;
            }
            '}' => {
                brace_count -= 1;
            }
            _ => {}
        }

        if found_start && brace_count == 0 {
            body_end_idx = body_start_idx + i;
            break;
        }
    }

    if body_end_idx > body_start_idx {
        // Return content inside braces, trimmed
        let body = &source[body_start_idx + 1 .. body_end_idx];
        return Some(body.trim().to_string());
    }

    None
}
