use anyhow::{Result, anyhow};
use ruchy::backend::Transpiler;
use ruchy::frontend::Parser;
use ruchy::frontend::ast::ExprKind;

pub struct RuchyOutput {
    pub setup: String,
    pub loop_body: String,
    pub variables: Vec<String>,
}

pub fn compile_ruchy_script(raw_source: &str) -> Result<RuchyOutput> {
    // FIX: Normalize Windows (CRLF) line endings to Unix (LF).
    // This looks for the specific "\r\n" sequence and makes it "\n".
    let source = raw_source.replace("\r\n", "\n");

    // 1. Parse
    let mut parser = Parser::new(&source);
    let ast = parser
        .parse()
        .map_err(|e| anyhow!("Failed to parse Ruchy code: {:?}", e))?;

    // 2. Transpile
    let mut transpiler = Transpiler::new();

    let mut setup_body = String::new();
    let mut loop_body = String::new();
    let mut variables = Vec::new();

    if let ExprKind::Block(exprs) = ast.kind {
        for expr in exprs {
            match expr.kind {
                ExprKind::Function { name, body, .. } => {
                    // Run Analysis
                    if let ExprKind::Block(ref stmts) = body.kind {
                        transpiler.analyze_mutability(stmts);
                    } else {
                        transpiler.analyze_mutability(&[body.as_ref().clone()]);
                    }

                    // Get raw tokens
                    let token_stream = transpiler.transpile_expr(&body)?;
                    let raw_code = token_stream.to_string();

                    // Format the code specifically for the target location
                    match name.as_str() {
                        "setup" => {
                            // Indent level 1
                            setup_body = format_rust_code(&raw_code, 1);
                        }
                        "forever" => {
                            // Indent level 2
                            loop_body = format_rust_code(&raw_code, 2);
                        }
                        _ => {}
                    }
                }
                _ => {
                    // It is a top-level expression (likely a variable definition)
                    // In espforge, we treat top-level let bindings as function-local variables in main()
                    transpiler.analyze_mutability(std::slice::from_ref(&expr));
                    let token_stream = transpiler.transpile_expr(&expr)?;
                    let raw_code = token_stream.to_string();

                    let mut stmt = raw_code.trim().to_string();

                    // Ensure semicolon
                    if !stmt.ends_with(';') {
                        stmt.push(';');
                    }

                    // Force 'mut' for state variables defined at top level so they can be updated in the loop
                    // Ruchy might output "let address = 1;" or "let mut address = 1;"
                    if stmt.starts_with("let ") {
                        if !stmt.starts_with("let mut ") {
                            stmt = stmt.replace("let ", "let mut ");
                        }
                    } else {
                        // Implicit declaration fallback "address = 1;" -> "let mut address = 1;"
                        stmt = format!("let mut {}", stmt);
                    }

                    variables.push(stmt);
                }
            }
        }
    }

    Ok(RuchyOutput {
        setup: setup_body,
        loop_body,
        variables,
    })
}

/// Cleans token stream artifacts and applies indentation/newlines
fn format_rust_code(input: &str, indent_level: usize) -> String {
    let indent = "    ".repeat(indent_level);

    // 1. Recursively remove outer braces from the block "{ ... }"
    // This fixes issues where transpiler wraps body in multiple blocks e.g. "{ { code } }"
    let mut content = input.trim();
    while content.starts_with('{') && content.ends_with('}') {
        content = content[1..content.len() - 1].trim();
    }

    // 2. Fix token spacing artifacts
    let mut cleaned = content.to_string();
    cleaned = cleaned.replace(" . ", ".");
    cleaned = cleaned.replace(" (", "(");
    cleaned = cleaned.replace(" )", ")");
    cleaned = cleaned.replace(" ;", ";");
    cleaned = cleaned.replace(" !", "!");
    cleaned = cleaned.replace(" :: ", "::");

    // 3. Split by semicolon to create newlines
    let mut formatted = String::new();
    let statements: Vec<&str> = cleaned
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for (i, stmt) in statements.iter().enumerate() {
        if i == 0 {
            // First line: No indentation (handled by template)
            formatted.push_str(&format!("{};", stmt));
        } else {
            // Subsequent lines: Newline + Indent + stmt
            formatted.push_str(&format!("\n{}{};", indent, stmt));
        }
    }

    formatted
}
